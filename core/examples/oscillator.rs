use apiary_core::{voct_to_frequency, AudioPacket, BLOCK_SIZE, CHANNELS, SAMPLE_RATE};
use std::f32::consts::PI;

use crate::display_module::{DisplayModule, Processor};

pub struct Oscillator {
    phase: [f32; CHANNELS],
    time: i64,
}

const NUM_PARAMS: usize = 0;

const IN_INPUT: usize = 0;
const NUM_INPUTS: usize = 1;

const SIN_OUTPUT: usize = 0;
const TRI_OUTPUT: usize = 1;
const SAW_OUTPUT: usize = 2;
const SQR_OUTPUT: usize = 3;
const NUM_OUTPUTS: usize = 4;

// const WT: usize = 2048;  // Wavetable size in samples

impl Oscillator {
    pub fn init() -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name("Oscillator")
            .input(IN_INPUT, "Input")
            .output(SIN_OUTPUT, "Sin")
            .output(TRI_OUTPUT, "Tri")
            .output(SAW_OUTPUT, "Saw")
            .output(SQR_OUTPUT, "Sqr")
            .start(Oscillator {
                phase: [0.0; CHANNELS],
                time: 0,
            })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Oscillator {
    fn process(
        &mut self,
        input: &[AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        _params: &[f32; NUM_PARAMS],
    ) {
        for i in 0..BLOCK_SIZE {
            for j in 0..CHANNELS {
                let a = 8000.0;
                output[SIN_OUTPUT].data[i].data[j] =
                    (a * (2.0 * PI * self.phase[j]).sin()).round() as i16;
                output[TRI_OUTPUT].data[i].data[j] = if self.phase[j] < 0.5 {
                    -a + 4.0 * a * self.phase[j]
                } else {
                    a - 4.0 * a * (self.phase[j] - 0.5)
                }
                .round() as i16;
                output[SAW_OUTPUT].data[i].data[j] = (-a + 2.0 * a * self.phase[j]).round() as i16;
                output[SQR_OUTPUT].data[i].data[j] =
                    if self.phase[j] < 0.5 { a } else { -a }.round() as i16;
                self.phase[j] += voct_to_frequency(input[IN_INPUT].data[i].data[j]) / SAMPLE_RATE;
                while self.phase[j] > 1.0 {
                    self.phase[j] -= 1.0;
                }
            }
        }
        self.time += 1
    }
}
