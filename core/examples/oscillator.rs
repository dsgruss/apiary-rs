use apiary_core::{voct_to_frequency, AudioPacket, BLOCK_SIZE, CHANNELS, SAMPLE_RATE};
use zerocopy::{AsBytes, FromBytes};
use std::{cmp::min, f32::consts::PI};

use crate::display_module::{DisplayModule, Processor};

pub struct Oscillator {
    osc: [WtOscillator; CHANNELS],
    level: f32,
}

const LEVEL_PARAM: usize = 0;
const RANGE_PARAM: usize = 1;
const NUM_PARAMS: usize = 2;

const IN_INPUT: usize = 0;
const LEVEL_INPUT: usize = 1;
const NUM_INPUTS: usize = 2;

const SIN_OUTPUT: usize = 0;
const TRI_OUTPUT: usize = 1;
const SAW_OUTPUT: usize = 2;
const SQR_OUTPUT: usize = 3;
const NUM_OUTPUTS: usize = 4;

impl Oscillator {
    pub fn init(name: &str) -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name(name)
            .input(IN_INPUT, "Input")
            .input(LEVEL_INPUT, "Level")
            .param(LEVEL_PARAM, 0.0, 1.0, 1.0, "Level", "", false)
            .param(RANGE_PARAM, -12.0, 12.0, 0.0, "Range", " semitones", false)
            .output(SIN_OUTPUT, "Sin")
            .output(TRI_OUTPUT, "Tri")
            .output(SAW_OUTPUT, "Saw")
            .output(SQR_OUTPUT, "Sqr")
            .start(Oscillator {
                osc: [Default::default(); CHANNELS],
                level: 0.0,
            })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Oscillator {
    fn process(
        &mut self,
        input: [&AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        params: &[f32; NUM_PARAMS],
    ) {
        for i in 0..BLOCK_SIZE {
            self.level += 0.0025 * (params[LEVEL_PARAM] - self.level);
            for j in 0..CHANNELS {
                let (sin, tri, saw, sqr) = self.osc[j].process(
                    input[IN_INPUT].data[i].data[j],
                    input[LEVEL_INPUT].data[i].data[j],
                    params[RANGE_PARAM],
                    self.level,
                );
                output[SIN_OUTPUT].data[i].data[j] = sin;
                output[TRI_OUTPUT].data[i].data[j] = tri;
                output[SAW_OUTPUT].data[i].data[j] = saw;
                output[SQR_OUTPUT].data[i].data[j] = sqr;
            }
        }
    }
}

#[derive(Copy, Clone, Default)]
struct NaiveOscillator {
    level: f32,
    phase: f32,
}

impl NaiveOscillator {
    fn process(&mut self, note: i16, level: i16, prange: f32, plevel: f32) -> (i16, i16, i16, i16) {
        self.level += 0.01 * (level as f32 - self.level);

        let a = self.level * plevel;

        let sin = (a * (2.0 * PI * self.phase).sin()).round() as i16;
        let tri = if self.phase < 0.5 {
            a * (-1.0 + 4.0 * self.phase)
        } else {
            a * (1.0 - 4.0 * (self.phase - 0.5))
        }
        .round() as i16;
        let saw = (-a + 2.0 * a * self.phase).round() as i16;
        let sqr = if self.phase < 0.5 { a } else { -a }.round() as i16;

        self.phase += voct_to_frequency(note as f32 + prange * 512.0) / SAMPLE_RATE;
        while self.phase > 1.0 {
            self.phase -= 1.0;
        }

        (sin, tri, saw, sqr)
    }
}

#[derive(Copy, Clone, Default)]
struct HarmOscillator {
    level: f32,
    phase: f32,
}

impl HarmOscillator {
    fn process(&mut self, note: i16, level: i16, prange: f32, plevel: f32) -> (i16, i16, i16, i16) {
        self.level += 0.01 * (level as f32 - self.level);

        let a = self.level * plevel;
        let freq = voct_to_frequency(note as f32 + prange * 512.0);
        let sin = a * (2.0 * PI * self.phase).sin();
        let mut tri = 0.0;
        let mut saw = 0.5;
        let mut sqr = 0.0;
        let nend = min((SAMPLE_RATE / (2.0 * freq)).floor() as u32, 100);
        for i in 1..nend {
            let n = i as f32;
            if i % 2 != 0 {
                if ((i - 1) / 2) % 2 == 0 {
                    tri += a * 8.0 / (PI * PI * n * n) * (n * 2.0 * PI * self.phase).sin();
                } else {
                    tri -= a * 8.0 / (PI * PI * n * n) * (n * 2.0 * PI * self.phase).sin();
                }
                sqr += a * 4.0 / (PI * n) * (n * 2.0 * PI * self.phase).sin();
            }
            saw -= a / (PI * n) * (n * 2.0 * PI * self.phase).sin();
        }

        self.phase += freq / SAMPLE_RATE;
        while self.phase > 1.0 {
            self.phase -= 1.0;
        }

        (
            sin.round() as i16,
            tri.round() as i16,
            saw.round() as i16,
            sqr.round() as i16,
        )
    }
}

// https://www.earlevel.com/main/2012/05/09/a-wavetable-oscillator-part-3/

#[derive(Copy, Clone, Debug)]
struct WtOscillator {
    level: f32,
    phase: f32,
}

lazy_static! {
    static ref WTSIN: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../wt/sin.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
    static ref WTTRI: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../wt/tri.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
    static ref WTSAW: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../wt/saw.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
    static ref WTSQR: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../wt/sqr.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
}

#[derive(AsBytes, FromBytes, Copy, Clone, Debug)]
#[repr(C)]
struct Wavetable {
    vals: [[f32; 2048]; 9],
}

impl Default for WtOscillator {
    fn default() -> Self {
        WtOscillator {
            level: 0.0,
            phase: 0.0,
        }
    }
}

impl WtOscillator {
    fn process(&mut self, note: i16, level: i16, prange: f32, plevel: f32) -> (i16, i16, i16, i16) {
        self.level += 0.01 * (level as f32 - self.level);

        let a = self.level * plevel;
        let freq = voct_to_frequency(note as f32 + prange * 512.0);

        let idx = match freq {
            f if f < 40.0 => 0,
            f if f < 80.0 => 0,
            f if f < 160.0 => 1,
            f if f < 320.0 => 2,
            f if f < 640.0 => 3,
            f if f < 1280.0 => 4,
            f if f < 2560.0 => 5,
            f if f < 5120.0 => 6,
            f if f < 10240.0 => 7,
            _ => 8,
        };

        let left = (self.phase * 2048.0).floor() as usize;
        let right = (self.phase * 2048.0).ceil() as usize % 2048;
        let frac = (self.phase * 2048.0) - (self.phase * 2048.0).floor();

        let sin = a * ((*WTSIN)[idx][left] * (1.0 - frac) + (*WTSIN)[idx][right] * frac);
        let tri = a * ((*WTTRI)[idx][left] * (1.0 - frac) + (*WTTRI)[idx][right] * frac);
        let saw = a * ((*WTSAW)[idx][left] * (1.0 - frac) + (*WTSAW)[idx][right] * frac);
        let sqr = a * ((*WTSQR)[idx][left] * (1.0 - frac) + (*WTSQR)[idx][right] * frac);

        self.phase += freq / SAMPLE_RATE;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        (
            sin.round() as i16,
            tri.round() as i16,
            saw.round() as i16,
            sqr.round() as i16,
        )
    }
}
