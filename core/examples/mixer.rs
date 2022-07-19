use apiary_core::{AudioPacket, BLOCK_SIZE, CHANNELS};

use crate::display_module::{DisplayModule, Processor};

pub struct Mixer;

const NUM_PARAMS: usize = 0;

const IN_INPUT: usize = 0;
const LEVEL_INPUT: usize = 1;
const NUM_INPUTS: usize = 2;

const MIX_OUTPUT: usize = 0;
const NUM_OUTPUTS: usize = 1;


impl Mixer {
    pub fn init() -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name("Mixer")
            .input(IN_INPUT, "Input")
            .input(LEVEL_INPUT, "Level")
            .output(MIX_OUTPUT, "Mix Out")
            .start(Mixer {})
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Mixer {
    fn process(
        &mut self,
        input: &[AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        _params: &[f32; NUM_PARAMS],
    ) {
        for i in 0..BLOCK_SIZE {
            for j in 0..CHANNELS {
                output[MIX_OUTPUT].data[i].data[j] = (input[IN_INPUT].data[i].data[j] as i32
                    * input[LEVEL_INPUT].data[i].data[j] as i32
                    >> 16) as i16;
            }
        }
    }
}
