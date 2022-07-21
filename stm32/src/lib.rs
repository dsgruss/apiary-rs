#![no_std]

#[macro_use]
extern crate log;

use stm32f4xx_hal::{gpio, gpio::Output};

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

    pub fn changed(&self) -> bool {
        self.just_pressed() || self.released()
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
            led_state: true,
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

pub struct UiPins {
    pub sw_sig2: gpio::Pin<'D', 12>,
    pub sw_light2: gpio::Pin<'D', 13>,
    pub sw_sig4: gpio::Pin<'C', 8>,
    pub sw_light4: gpio::Pin<'C', 9>,
}

pub struct Ui {
    sw_sig2: Switch<'D', 12>,
    sw_light2: Led<'D', 13>,
    sw_sig4: Switch<'C', 8>,
    sw_light4: Led<'C', 9>,
}

// TIM2 CH1 : PA15 Red
// TIM2 CH2 : PB3 Blue
// TIM3 CH1 : PB4 Green

impl Ui {
    pub fn new(pins: UiPins) -> Ui {
        Ui {
            sw_sig2: Switch::new(pins.sw_sig2),
            sw_light2: Led::new(pins.sw_light2),
            sw_sig4: Switch::new(pins.sw_sig4),
            sw_light4: Led::new(pins.sw_light4),
        }
    }

    pub fn poll(&mut self) -> (bool, bool, bool) {
        self.sw_sig2.debounce();
        self.sw_sig4.debounce();
        if self.sw_sig2.just_pressed() {
            info!("SW2 switch pressed");
            self.sw_light2.toggle();
        }
        if self.sw_sig2.released() {
            info!("SW2 switch released");
        }
        if self.sw_sig4.just_pressed() {
            info!("SW4 switch pressed");
            self.sw_light4.toggle();
        }
        if self.sw_sig4.released() {
            info!("SW4 switch released");
        }

        (
            self.sw_sig2.changed() || self.sw_sig4.changed(),
            self.sw_sig2.just_pressed(),
            self.sw_sig4.just_pressed(),
        )
    }
}
