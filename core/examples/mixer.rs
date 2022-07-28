use apiary_core::AudioPacket;
use itertools::izip;

use crate::display_module::{DisplayModule, Processor};

pub struct Mixer;

const SCALE_PARAM: usize = 0;
const NUM_PARAMS: usize = 1;

const IN0_INPUT: usize = 0;
const LEVEL0_INPUT: usize = 1;
const IN1_INPUT: usize = 2;
const LEVEL1_INPUT: usize = 3;
const IN2_INPUT: usize = 4;
const LEVEL2_INPUT: usize = 5;
const NUM_INPUTS: usize = 6;

const MIX_OUTPUT: usize = 0;
const NUM_OUTPUTS: usize = 1;

impl Mixer {
    pub fn init(name: &str) -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name(name)
            .input(IN0_INPUT, "Input 0")
            .input(LEVEL0_INPUT, "Level 0")
            .input(IN1_INPUT, "Input 1")
            .input(LEVEL1_INPUT, "Level 1")
            .input(IN2_INPUT, "Input 2")
            .input(LEVEL2_INPUT, "Level 2")
            .param(SCALE_PARAM, 0.0, 100.0, 100.0, "Scale", "%", false)
            .output(MIX_OUTPUT, "Mix Out")
            .start(Mixer {})
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Mixer {
    fn process(
        &mut self,
        input: [&AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        params: &[f32; NUM_PARAMS],
    ) {
        for (in0, l0, in1, l1, in2, l2, o) in izip!(
            input[IN0_INPUT].data,
            input[LEVEL0_INPUT].data,
            input[IN1_INPUT].data,
            input[LEVEL1_INPUT].data,
            input[IN2_INPUT].data,
            input[LEVEL2_INPUT].data,
            output[MIX_OUTPUT].data.iter_mut()
        ) {
            for (fin0, fl0, fin1, fl1, fin2, fl2, fo) in izip!(
                in0.data,
                l0.data,
                in1.data,
                l1.data,
                in2.data,
                l2.data,
                o.data.iter_mut()
            ) {
                *fo = ((fin0 as i32 * fl0 as i32 / 100 * params[SCALE_PARAM] as i32
                    + fin1 as i32 * fl1 as i32 / 100 * params[SCALE_PARAM] as i32
                    + fin2 as i32 * fl2 as i32 / 100 * params[SCALE_PARAM] as i32)
                    >> 16) as i16;
            }
        }
    }
}
