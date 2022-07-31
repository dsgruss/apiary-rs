use stm32f4xx_hal::{
    gpio,
    gpio::{Output},
};

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
    pub input: gpio::Pin<'C', 8>,
    pub key_track: gpio::Pin<'C', 9>,
    pub contour: gpio::Pin<'D', 12>,
    pub output: gpio::Pin<'D', 13>,
}

pub struct Ui {
    input: Switch<'C', 8>,
    key_track: Switch<'C', 9>,
    contour: Switch<'D', 12>,
    output: Switch<'D', 13>,
}

impl Ui {
    pub fn new(pins: UiPins) -> Ui {
        Ui {
            input: Switch::new(pins.input),
            key_track: Switch::new(pins.key_track),
            contour: Switch::new(pins.contour),
            output: Switch::new(pins.output),
        }
    }

    pub fn poll(&mut self) -> UiUpdate {
        self.input.debounce();
        self.key_track.debounce();
        self.contour.debounce();
        self.output.debounce();
        if self.input.just_pressed() {
            info!("input switch pressed");
        }
        if self.input.released() {
            info!("input switch released");
        }
        if self.key_track.just_pressed() {
            info!("key_track switch pressed");
        }
        if self.key_track.released() {
            info!("key_track switch released");
        }
        if self.contour.just_pressed() {
            info!("contour switch pressed");
        }
        if self.contour.released() {
            info!("contour switch released");
        }
        if self.output.just_pressed() {
            info!("output switch pressed");
        }
        if self.output.released() {
            info!("output switch released");
        }

        UiUpdate {
            changed: self.input.changed()
                || self.key_track.changed()
                || self.contour.changed()
                || self.output.changed(),
            input_pressed: self.input.just_pressed(),
            key_track_pressed: self.key_track.just_pressed(),
            contour_pressed: self.contour.just_pressed(),
            output_pressed: self.output.just_pressed(),
        }
    }
}

pub struct UiUpdate {
    pub changed: bool,
    pub input_pressed: bool,
    pub key_track_pressed: bool,
    pub contour_pressed: bool,
    pub output_pressed: bool,
}
