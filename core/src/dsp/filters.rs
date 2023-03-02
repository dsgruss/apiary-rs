use core::f32::consts::PI;

use fixed::types::{I17F15, I1F15, I4F12, I4F28, I5F27, U4F12};
use libm::{roundf, sinf, tanf};

use crate::{softclip, SAMPLE_RATE};

const PI_2: f32 = PI * PI;
const PI_3: f32 = PI * PI_2;

// https://www.native-instruments.com/fileadmin/ni_media/downloads/pdf/VAFilterDesign_1.1.1.pdf

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
    omega0dt: I1F15,
    state: [I1F15; 4],
    resonance: I17F15,
    input: I1F15,
}

impl Default for LadderFilterFP {
    fn default() -> Self {
        LadderFilterFP {
            omega0dt: I1F15::from_num(0.15_f32),
            state: [I1F15::from_num(0_i16); 4],
            resonance: I17F15::from_num(0_i16),
            input: I1F15::from_num(0_i16),
        }
    }
}

pub fn softclipfp(x: I17F15) -> I1F15 {
    let three = I17F15::from_num(3_i16);
    let nine = three * three;
    let twenty_seven = nine * three;
    let y = if x < -three {
        -three
    } else if x > three {
        three
    } else {
        x
    };
    I1F15::from_num(y * (twenty_seven + y * y) / (twenty_seven + nine * y * y))
}

impl LadderFilterFP {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.omega0dt = I1F15::from_num((2.0 * PI * cutoff / SAMPLE_RATE).clamp(0.0, 0.99));
        self.resonance = I17F15::from_num(resonance);
    }

    pub fn process(&mut self, input: i16) -> i16 {
        let mut state = self.state.clone();
        self.rk4(&mut state, self.input, I1F15::from_bits(input));
        self.state = state;
        self.input = I1F15::from_bits(input);
        self.state[3].to_bits()
    }

    fn f(&self, x: [I1F15; 4], inputt: I1F15) -> [I1F15; 4] {
        // let mut dxdt = [0; 4];
        // let inputt = input * (t / dt) + input_new * (1.0 - t / dt);
        let inputc = softclipfp(I17F15::from(inputt) - self.resonance * I17F15::from(x[3]));
        // let yc = x.map(softclip);
        // let inputc = inputt;
        let yc = x;

        [
            self.omega0dt * softclipfp(I17F15::from(inputc) - I17F15::from(yc[0])),
            self.omega0dt * softclipfp(I17F15::from(yc[0]) - I17F15::from(yc[1])),
            self.omega0dt * softclipfp(I17F15::from(yc[1]) - I17F15::from(yc[2])),
            self.omega0dt * softclipfp(I17F15::from(yc[2]) - I17F15::from(yc[3])),
        ]
    }

    fn rk4(&mut self, x: &mut [I1F15; 4], input: I1F15, input_new: I1F15) {
        let mut yi = [I1F15::from_num(0); 4];

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
        self.g = tanf(PI * cutoff.clamp(20.0, 8000.0) / SAMPLE_RATE);
        self.r = (10.0 - resonance) / 10.0;
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

#[derive(Default)]
pub struct NaiveSvf {
    f: f32,
    damp: f32,
    lp: f32,
    bp: f32,
}

impl NaiveSvf {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.f = 2.0 * sinf(PI * cutoff.clamp(20.0, 8000.0) / SAMPLE_RATE);
        self.damp = (10.0 - resonance) / 5.0;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let bp_normalized = self.bp * self.damp;
        let notch = input - bp_normalized;
        self.lp += self.f * self.bp;
        let hp = notch - self.lp;
        self.bp += self.f * hp;
        self.lp
    }
}

// https://www.cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf
#[derive(Default)]
pub struct LinearTrap {
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic2eq: f32,
    ic1eq: f32,
}

impl LinearTrap {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        // self.g = tanf(PI * cutoff.clamp(20.0, 8000.0) / SAMPLE_RATE);
        let f = cutoff.clamp(20.0, 8000.0) / SAMPLE_RATE;
        self.g = f * (PI + 3.736e-1 * PI_3 * f * f);
        self.k = 2.0 - 2.0 * (resonance / 10.0);
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }
    pub fn process(&mut self, v0: f32) -> f32 {
        let v3 = v0 - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        v2
    }
}
