use apiary_core::{voct_to_frequency, AudioPacket, BLOCK_SIZE, CHANNELS, SAMPLE_RATE};
use std::f32::consts::PI;

use crate::display_module::{DisplayModule, Processor};

pub struct Oscillator {
    osc: [NaiveOscillator; CHANNELS],
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
            a * (1.0 - 4.0 * self.phase)
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
