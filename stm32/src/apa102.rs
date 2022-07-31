//! # Use apa102 leds via spi
//!
//! - For usage with `smart-leds`
//! - Implements the `SmartLedsWrite` trait
//!
//! Adapted from https://github.com/smart-leds-rs/apa102-spi-rs as the cargo crate is not the latest
//! version with PixelOrder (as of 7/30/2022). Using the native brightness settings of the apa102
//! leds runs at a much lower pwm frequency and thus nerfes the very high color pwm frequency.
//! (According to Adafruit)
//!
//! Needs a type implementing the `blocking::spi::Write` trait.

use embedded_hal::blocking::spi::Write;
use embedded_hal::spi::{Mode, Phase, Polarity};
use palette::Srgb;

/// SPI mode that is needed for this crate
///
/// Provided for convenience
pub const MODE: Mode = Mode {
    polarity: Polarity::IdleLow,
    phase: Phase::CaptureOnFirstTransition,
};

pub struct Apa102<SPI> {
    spi: SPI,
    end_frame_length: u8,
    invert_end_frame: bool,
    pixel_order: PixelOrder,
    global_intensity: u8,
}

/// What order to transmit pixel colors. Different Dotstars
/// need their pixel color data sent in different orders.
pub enum PixelOrder {
    RGB,
    RBG,
    GRB,
    GBR,
    BRG,
    BGR, // Default
}

impl<SPI, E> Apa102<SPI>
where
    SPI: Write<u8, Error = E>,
{
    /// new constructs a controller for a series of APA102 LEDs. By default, an End Frame consisting
    /// of 32 bits of zeroes is emitted following the LED data. Control over the size and polarity
    /// of the End Frame and the pixel ordering (default BGR) is possible using the builder
    /// functions.
    pub fn new(spi: SPI) -> Apa102<SPI> {
        Self {
            spi,
            end_frame_length: 4,
            invert_end_frame: false,
            pixel_order: PixelOrder::BGR,
            global_intensity: 0xFF,
        }
    }

    pub fn end_frame_length(mut self, end_frame_length: u8) -> Self {
        self.end_frame_length = end_frame_length;
        self
    }

    pub fn invert_end_frame(mut self, invert_end_frame: bool) -> Self {
        self.invert_end_frame = invert_end_frame;
        self
    }

    pub fn pixel_order(mut self, pixel_order: PixelOrder) -> Self {
        self.pixel_order = pixel_order;
        self
    }

    /// Set the global intensity of all the leds
    pub fn set_intensity(&mut self, intensity: u8) {
        self.global_intensity = 0xE0 + (intensity >> 3);
    }

    /// Write all the items of an iterator to an apa102 strip
    pub fn write<T>(&mut self, iterator: T) -> Result<(), E>
    where
        T: Iterator<Item = Srgb<u8>>,
    {
        self.spi.write(&[0x00, 0x00, 0x00, 0x00])?;
        let glob = self.global_intensity;
        for item in iterator {
            match self.pixel_order {
                PixelOrder::RGB => self.spi.write(&[glob, item.red, item.green, item.blue])?,
                PixelOrder::RBG => self.spi.write(&[glob, item.red, item.blue, item.green])?,
                PixelOrder::GRB => self.spi.write(&[glob, item.green, item.red, item.blue])?,
                PixelOrder::GBR => self.spi.write(&[glob, item.green, item.blue, item.red])?,
                PixelOrder::BRG => self.spi.write(&[glob, item.blue, item.red, item.green])?,
                PixelOrder::BGR => self.spi.write(&[glob, item.blue, item.green, item.red])?,
            }
        }
        for _ in 0..self.end_frame_length {
            match self.invert_end_frame {
                false => self.spi.write(&[0xFF])?,
                true => self.spi.write(&[0x00])?,
            };
        }
        Ok(())
    }
}
