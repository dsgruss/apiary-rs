use apiary_core::{dsp::oscillators::WtOscillator, AudioPacket, BLOCK_SIZE, CHANNELS};

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
