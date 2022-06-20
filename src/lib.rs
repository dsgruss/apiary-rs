#![no_std]

use stm32_eth::hal::gpio;
use stm32_eth::hal::gpio::Output;

pub struct Switch<const P: char, const N: u8> {
    pin: gpio::Pin<P, N>,
    switch_state: u8,
}

impl<const P: char, const N: u8> Switch<P, N> {
    pub fn new(pin: gpio::Pin<P, N>) -> Switch<P, N> {
        Switch {
            pin: pin.into_pull_up_input(),
            switch_state: 0xff,
        }
    }

    pub fn debounce(&mut self) {
        self.switch_state = (self.switch_state << 1) | (self.pin.is_high() as u8);
    }

    pub fn released(&self) -> bool {
        self.switch_state == 0x7f
    }

    pub fn just_pressed(&self) -> bool {
        self.switch_state == 0x80
    }

    pub fn pressed(&self) -> bool {
        self.switch_state == 0x00
    }
}

pub struct Led<const P: char, const N: u8> {
    pin: gpio::Pin<P, N, Output>,
    led_state: bool,
}

impl<const P: char, const N: u8> Led<P, N> {
    pub fn new(pin: gpio::Pin<P, N>) -> Led<P, N> {
        Led {
            pin: pin.into_push_pull_output(),
            led_state: false,
        }
    }

    pub fn toggle(&mut self) {
        if self.led_state {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
        self.led_state = !self.led_state;
    }
}
