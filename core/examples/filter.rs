use apiary_core::{
    dsp::filters::LinearTrap, voct_to_freq_scale, AudioPacket, BLOCK_SIZE, CHANNELS,
};
use rand::Rng;

use crate::display_module::{DisplayModule, Processor};

pub struct Filter {
    filters: [LinearTrap; CHANNELS],
}

const FREQ_PARAM: usize = 0;
const RES_PARAM: usize = 1;
const CONTOUR_PARAM: usize = 2;
const NUM_PARAMS: usize = 3;

const IN_INPUT: usize = 0;
const KEY_INPUT: usize = 1;
const CONTOUR_INPUT: usize = 2;
const NUM_INPUTS: usize = 3;

const LPF_OUTPUT: usize = 0;
const NUM_OUTPUTS: usize = 1;

impl Filter {
    pub fn init() -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name("Filter")
            .param(FREQ_PARAM, 20.0, 8000.0, 4000.0, "Cutoff", " Hz", true)
            .param(RES_PARAM, 0.0, 1.0, 0.75, "Resonance", "", false)
            .param(CONTOUR_PARAM, 0.0, 100.0, 0.0, "Contour", "%", false)
            .input(IN_INPUT, "Audio")
            .input(KEY_INPUT, "Key Track")
            .input(CONTOUR_INPUT, "Contour")
            .output(LPF_OUTPUT, "Lowpass Filter")
            .start(Filter {
                filters: Default::default(),
            })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Filter {
    fn process(
        &mut self,
        input: [&AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        params: &[f32; NUM_PARAMS],
    ) {
        let mut rng = rand::thread_rng();
        for i in 0..BLOCK_SIZE {
            for j in 0..CHANNELS {
                self.filters[j].set_params(
                    params[FREQ_PARAM]
                        * voct_to_freq_scale(
                            input[KEY_INPUT].data[i].data[j] as f32
                                + input[CONTOUR_INPUT].data[i].data[j] as f32 / i16::MAX as f32
                                    * params[CONTOUR_PARAM]
                                    / 100.0
                                    * 512.0
                                    * 12.0
                                    * 4.0,
                        ),
                    params[RES_PARAM].powi(2) * 10.0,
                );
                output[LPF_OUTPUT].data[i].data[j] = (self.filters[j].process(
                    input[IN_INPUT].data[i].data[j] as f32 / i16::MAX as f32
                        + rng.gen_range(-1e-6..1e-6),
                    // 1.0 / SAMPLE_RATE,
                ) * i16::MAX as f32)
                    .round() as i16;
            }
        }
    }
}
