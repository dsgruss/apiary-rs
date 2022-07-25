use core::f32::consts::PI;

use libm::{roundf, tanf};

use crate::{softclip, SAMPLE_RATE};

pub struct LadderFilter {
    omega0dt: f32,
    state: [f32; 4],
    resonance: f32,
    input: f32,
}

impl Default for LadderFilter {
    fn default() -> Self {
        LadderFilter {
            omega0dt: 2.0 * PI * 1000.0 / SAMPLE_RATE,
            state: [0.0; 4],
            resonance: 1.0,
            input: 0.0,
        }
    }
}

impl LadderFilter {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.omega0dt = 2.0 * PI * cutoff.clamp(0.0, 8000.0) / SAMPLE_RATE;
        self.resonance = resonance;
    }

    pub fn process(&mut self, input: f32, _dt: f32) -> f32 {
        let mut state = self.state.clone();
        self.rk4(&mut state, self.input, input);
        self.state = state;
        self.input = input;
        self.state[3]
    }

    fn f(&self, x: [f32; 4], inputt: f32) -> [f32; 4] {
        let mut dxdt = [0.0; 4];
        // let inputt = input * (t / dt) + input_new * (1.0 - t / dt);
        let inputc = softclip(inputt - self.resonance * x[3]);
        let yc = x.map(softclip);

        dxdt[0] = self.omega0dt * (inputc - yc[0]);
        dxdt[1] = self.omega0dt * (yc[0] - yc[1]);
        dxdt[2] = self.omega0dt * (yc[1] - yc[2]);
        dxdt[3] = self.omega0dt * (yc[2] - yc[3]);
        dxdt
    }

    fn rk4(&mut self, x: &mut [f32; 4], input: f32, input_new: f32) {
        let mut yi = [0.0; 4];

        let k1 = self.f(*x, input_new);
        for i in 0..4 {
            yi[i] = x[i] + k1[i] * 0.5;
        }
        let k2 = self.f(yi, (input + input_new) * 0.5);
        for i in 0..4 {
            yi[i] = x[i] + k2[i] * 0.5;
        }
        let k3 = self.f(yi, (input + input_new) * 0.5);
        for i in 0..4 {
            yi[i] = x[i] + k3[i];
        }
        let k4 = self.f(yi, input);
        for i in 0..4 {
            x[i] += (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]) / 6.0;
        }
    }
}

pub struct LadderFilterFP {
    omega0dt: i16,
    state: [i16; 4],
    resonance: f32,
    input: i16,
}

impl Default for LadderFilterFP {
    fn default() -> Self {
        LadderFilterFP {
            omega0dt: roundf(2.0 * PI * 1000.0 / SAMPLE_RATE * i16::MAX as f32) as i16,
            state: [0; 4],
            resonance: 0.0,
            input: 0,
        }
    }
}

fn fpmul(x: i16, y: i16) -> i16 {
    ((x as i32 * y as i32) >> 16) as i16
}

impl LadderFilterFP {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.omega0dt = roundf(2.0 * PI * cutoff.clamp(0.0, 8000.0) / SAMPLE_RATE * i16::MAX as f32) as i16;
        self.resonance = resonance;
    }

    pub fn process(&mut self, input: i16, _dt: i16) -> i16 {
        let mut state = self.state.clone();
        self.rk4(&mut state, self.input, input);
        self.state = state;
        self.input = input;
        self.state[3]
    }

    fn f(&self, x: [i16; 4], inputt: i16) -> [i16; 4] {
        let mut dxdt = [0; 4];
        // let inputt = input * (t / dt) + input_new * (1.0 - t / dt);
        // let inputc = (fastclamp(inputt as f32 - self.resonance * x[3] as f32) * i16::MAX as f32) as i16;
        // let yc = x.map(softclip);
        let inputc = inputt;
        let yc = x;

        dxdt[0] = fpmul(self.omega0dt, inputc - yc[0]);
        dxdt[1] = fpmul(self.omega0dt, yc[0] - yc[1]);
        dxdt[2] = fpmul(self.omega0dt, yc[1] - yc[2]);
        dxdt[3] = fpmul(self.omega0dt, yc[2] - yc[3]);
        dxdt
    }

    fn rk4(&mut self, x: &mut [i16; 4], input: i16, input_new: i16) {
        let mut yi = [0; 4];

        let k1 = self.f(*x, input_new);
        for i in 0..4 {
            yi[i] = x[i] + (k1[i] >> 1);
        }
        let k2 = self.f(yi, (input >> 1) + (input_new >> 1));
        for i in 0..4 {
            yi[i] = x[i] + (k2[i] >> 1);
        }
        let k3 = self.f(yi, (input >> 1) + (input_new >> 1));
        for i in 0..4 {
            yi[i] = x[i] + k3[i];
        }
        let k4 = self.f(yi, input);
        for i in 0..4 {
            x[i] += k1[i] / 6 + k2[i] / 3 + k3[i] / 3 + k4[i] / 6;
        }
    }
}

#[derive(Default)]
pub struct Svf {
    g: f32,
    r: f32,
    h: f32,
    state_1: f32,
    state_2: f32,
}

impl Svf {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.g = tanf(PI * cutoff / SAMPLE_RATE);
        self.r = 1.0 / (resonance + 1.0);
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let hp = (input - self.r * self.state_1 - self.g * self.state_1 - self.state_2) * self.h;
        let bp = self.g * hp + self.state_1;
        self.state_1 = self.g * hp + bp;
        let lp = self.g * bp + self.state_2;
        self.state_2 = self.g * bp + lp;
        lp
    }
}