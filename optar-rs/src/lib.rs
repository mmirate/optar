extern crate image;
extern crate num;
use std::str::FromStr;

#[derive(Copy, Clone, Debug)] pub enum FecOrder { Golay, Hamming(u8) }
impl FecOrder {
    pub fn large_bits(&self) -> u64 { match *self { FecOrder::Golay => 24, FecOrder::Hamming(x) => 1<<x } }
    pub fn small_bits(&self) -> u64 { match *self { FecOrder::Golay => 12, FecOrder::Hamming(x) => self.large_bits()-1-(x as u64) } }
}
impl Default for FecOrder { fn default() -> Self { FecOrder::Golay } }
impl From<u8> for FecOrder { fn from(x: u8) -> FecOrder { if x == 1 { FecOrder::Golay } else { FecOrder::Hamming(x) } } }
impl Into<u8> for FecOrder { fn into(self) -> u8 { match self { FecOrder::Golay => 1, FecOrder::Hamming(x) => x } } }

pub struct Settings {
    border: u64, /* In pixels. Thickness of the border */
    chalf: u64, /* Size of the cross half. Size of the cross is CHALF*2 x CHALF*2. */
    cpitch: u64, /* Distance between cross centers */
    text_width: u64,
    text_height: u64,
    xcrosses: u64, /* Number of crosses horizontally */
    ycrosses: u64, /* Number of crosses vertically */
    fec_order: FecOrder,
}

impl Default for Settings { fn default() -> Settings {
    Settings { border: 2, chalf: 3, cpitch: 24, text_width: 13, text_height: 24, xcrosses: 67, ycrosses: 87, fec_order: FecOrder::Golay }
} }
impl FromStr for Settings {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> std::result::Result<Settings, Self::Err> {
        let components : Vec<&str> = s.split("-").collect();
        Ok(Settings { xcrosses: components[1].parse()?, ycrosses: components[2].parse()?, cpitch: components[3].parse()?, chalf: components[4].parse()?, fec_order: components[5].parse::<u8>()?.into(), border: components[6].parse()?, text_height: components[7].parse()?, .. Settings::default() })
    }
}

impl Settings {
    /* The rectangle occupied by the data and crosses */
    fn data_width(&self) -> u64 { self.cpitch*(self.xcrosses-1)+2*self.chalf }
    fn data_height(&self) -> u64 { self.cpitch*(self.ycrosses-1)+2*self.chalf }
    fn width(&self) -> u64 { 2*self.border+self.data_width() }
    fn height(&self) -> u64 { 2*self.border+self.data_height()+self.text_height }

    /* Properties of the narrow horizontal strip, with crosses */
    fn narrow_height(&self) -> u64 { 2*self.chalf }
    fn gap_width(&self) -> u64 { self.cpitch-2*self.chalf }
    fn narrow_width(&self) -> u64 { self.gap_width()*(self.xcrosses-1) }
    fn narrow_pixels(&self) -> u64 { self.narrow_height()*self.narrow_width() }

    /* Properties of the wide horizontal strip, without crosses */
    fn wide_height(&self) -> u64 { self.gap_width() }
    fn wide_width(&self) -> u64 { self.width()-2*self.border }
    fn wide_pixels(&self) -> u64 { self.wide_height()*self.wide_width() }

    /* Amount of raw payload pixels in one narrow-wide strip pair */
    fn rep_height(&self) -> u64 { self.narrow_height() + self.wide_height() }
    fn rep_pixels(&self) -> u64 { self.narrow_pixels() + self.wide_pixels() }

    /* Total bits before hamming including the unused */
    fn total_bits(&self) -> u64 { self.rep_pixels()*(self.ycrosses-1)+self.narrow_pixels() }

    /* Hamming net channel capacity */
    fn fec_syms(&self) -> u64 { self.total_bits() / self.fec_order.large_bits() }
    fn net_bits(&self) -> u64 { self.fec_syms()*self.fec_order.small_bits() }
    fn used_bits(&self) -> u64 { self.fec_syms()*self.fec_order.large_bits() }

    /* Coordinates don't count with the border - 0,0 is upper left corner of the
     * first cross! */
    fn is_cross(&self, x: u64, y: u64) -> bool {
        ((x%self.cpitch) < (2*self.chalf)) && ((y%self.cpitch) < (2*self.chalf))
    }

    /* Returns the coords relative to the upperloeftmost cross upper left corner
     * pixel! If you have borders, you have to add them! */

    fn seq2xy(&self, seq: u64) -> Option<(u64, u64)> {
        if seq >= self.total_bits() { return None }
        let rep = seq/self.rep_pixels(); /* Repetition - number of narrow strip - wide strip pair, starting with 0 */
        let mut seq = seq%self.rep_pixels();

        let mut y = self.rep_height() * rep;
        /* Now seq is sequence in the repetition pair */
        if seq >= self.narrow_pixels() {
            /* Second, wide strip of the pair */
            y += self.narrow_height();
            seq -= self.narrow_pixels();
		    /* Now seq is sequence in the wide strip */
            y += seq / self.wide_width();
            let x = seq % self.wide_width();
            Some((x,y))
        } else {
            /* First, narrow strip of the pair */
            let mut x = 2 * self.chalf;
            y += seq / self.narrow_width();
            seq %= self.narrow_width();
		    /* seq is now sequence in the horiz. line */
            let gap = seq / self.gap_width(); /* Horizontal gap number */
            x += gap * self.cpitch;
            seq %= self.gap_width();
            /* seq is now sequence in the gap */
            x += seq;
            Some((x,y))
        }
    }
}

pub fn parity(mut input: u64) -> u64 {
    let mut bit = (u64::max_value().count_ones()>>1);
    while bit > 0 {
        input ^= input >> bit;
        bit >>= 1;
    }
    input & 1
}

pub struct OptarWriter { buffer: image::ImageBuffer<image::Luma<u8>, Vec<u8>>, settings: Settings, accu: u64, hamming_symbol: u64, base_filename: String, file_number: u16 }
impl OptarWriter {
    fn new(settings: Settings, base_filename: Option<String>) -> OptarWriter {
        OptarWriter { buffer: image::ImageBuffer::from_pixel(settings.width() as u32, settings.height() as u32, image::Luma([255u8])), settings: settings, accu: 1, hamming_symbol: 0, base_filename: base_filename.unwrap_or("optar_out".to_owned()), file_number: 0 }
    }

    fn write_output(&mut self) -> std::io::Result<()> {
        image::save_buffer(format!("{}_{:04}.png", self.base_filename, self.file_number), &self.buffer, self.settings.width() as u32, self.settings.height() as u32, image::ColorType::Gray(0))
    }
    /* Groups into two groups of bits, 0...bit-1 and bit..., and then makes
     * a gap with zero between them by shifting the higer bits up. */
    fn split(mut input: u64, bit: u8) -> u64 {
        let mut high = input;
        input &= (1u64<<bit)-1;
        high ^= input;
        (high << 1) | input
    }
    /* Thie bits are always stored in the LSB side of the register. Only the
     * lowest FEC_SMALLBITS are taken into account on input. */
    fn hamming(mut input: u64, order: u8) -> u64 {
        input &= (1u64 << FecOrder::Hamming(order).small_bits()) - 1;
        input <<= 3; /* Split 0,1,2 */
        if order >= 3 {
            for bit in 3..(order+1) {
                input = Self::split(input, (1<<(bit-1)));
            }
        }
        for bit in (1..(order+1)).rev() {
            let x = 1<<(bit-1);
            let mask = u64::from_str_radix({
                let unit = "1".repeat(x) + &"0".repeat(x);
                &unit.repeat((u64::max_value().count_ones() as usize)/unit.len())
            }, 2).unwrap();
            input |= parity(input&mask)<<x;
        }
        input |= parity(input);
        input
    }
    fn border(&mut self) {
        for x in 0..self.buffer.width() {
            for y in 0..self.buffer.height() {
                if x <= (self.settings.border as u32) || x >= self.buffer.width() - (self.settings.border as u32) || y <= (self.settings.border as u32) || y >= self.buffer.width() - (self.settings.border as u32) - (self.settings.text_height as u32) {
                    self.buffer.put_pixel(x, y, image::Luma([0u8]));
                }
            }
        }
    }
    fn cross(&mut self, x: u32, y: u32) {
        for i in x..x+(self.settings.chalf as u32) {
            for j in y..y+(self.settings.chalf as u32) {
                self.buffer.put_pixel(i, j, image::Luma([0u8]));
                self.buffer.put_pixel(i+(self.settings.chalf as u32), j, image::Luma([255u8]));
                self.buffer.put_pixel(i, j+(self.settings.chalf as u32), image::Luma([255u8]));
                self.buffer.put_pixel(i+(self.settings.chalf as u32), j+(self.settings.chalf as u32), image::Luma([0u8]));
            }
        }
    }
    fn crosses(&mut self) {
        for y in num::range_step(self.settings.border, self.settings.height()-self.settings.text_height-self.settings.border-2*self.settings.chalf, self.settings.cpitch) {
            for x in num::range_step(self.settings.border, self.settings.width()-self.settings.border-2*self.settings.chalf, self.settings.cpitch) {
                self.cross(x as u32, y as u32);
            }
        }
    }
    fn reformat_buffer(&mut self) {
        self.buffer = image::ImageBuffer::from_pixel(self.settings.width() as u32, self.settings.height() as u32, image::Luma([255u8]));
        self.border();
        self.crosses();
        //self.label();
    }
    fn new_file(&mut self) -> std::io::Result<()> {
        if self.file_number > 0 { self.write_output()?; }
        assert!(self.file_number < 9999);
        self.file_number += 1;
        self.reformat_buffer();
        Ok(())
    }
    /* Only the LSB is significant. Writes hamming-encoded bits. The sequence number
     * must not be out of range! */
    fn write_channelbit(&mut self, mut bit: u8, seq: u64) {
        bit &= 1u8;
        bit.wrapping_sub(1);
        /* White=bit 0, black=bit 1 */
        let (x, y) = self.settings.seq2xy(seq).unwrap();
        // INCOMPLETE
        self.buffer.put_pixel((x+self.settings.border) as u32, (y+self.settings.border) as u32, image::Luma([bit]));
        //CHANGE THIS self.buffer[(x+self.settings.border)+(y+self.settings.border)*self.settings.width()] = bit;
    }

    /* That's the net channel capacity */
    fn write_payloadbit(&mut self, bit: u8) -> std::io::Result<()> {
        self.accu <<= 1;
        self.accu |= (bit&1u8) as u64;
        if self.accu&(1u64<<self.settings.fec_order.small_bits()) != 0 {
            match self.settings.fec_order {
                FecOrder::Golay => unimplemented!(),
                FecOrder::Hamming(x) => self.accu = Self::hamming(self.accu, x),
            }
            if self.hamming_symbol >= self.settings.fec_syms() {
                self.new_file()?;
                self.hamming_symbol = 0;
            }
            for shift in (0..self.settings.fec_order.large_bits()).rev() {
                let bit = (self.accu>>shift) as u8;
                let seq = self.hamming_symbol+(self.settings.fec_order.large_bits()-1-shift)*self.settings.fec_syms();
                self.write_channelbit(bit, seq);
            }
            self.accu=1;
            self.hamming_symbol += 1;
        }
        Ok(())
    }

    fn write_byte(&mut self, c: u8) -> std::io::Result<()>  {
        for bit in (0..8).rev() {
            self.write_payloadbit(c>>bit)?;
        }
        Ok(())
    }

    fn feed_data<R: std::io::Read>(&mut self, input_stream: R) -> std::io::Result<()> {
        for c in input_stream.bytes() { self.write_byte(c?); }
        /* Flush the FEC with zeroes */
        for c in 1..self.settings.fec_order.small_bits() { self.write_payloadbit(0); }
        self.write_output()
    }
}