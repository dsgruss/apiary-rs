use std::collections::VecDeque;

use apiary_core::AudioPacket;
use itertools::izip;

use crate::display_module::{DisplayModule, Processor};

pub struct Reverb {
    buffer: VecDeque<AudioPacket>,
}

const WET_PARAM: usize = 0;
const TIME_PARAM: usize = 1;
const NUM_PARAMS: usize = 2;

const IN_INPUT: usize = 0;
const NUM_INPUTS: usize = 1;

const OUT_OUTPUT: usize = 0;
const NUM_OUTPUTS: usize = 1;

impl Reverb {
    pub fn init(name: &str) -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        let mut buffer = VecDeque::with_capacity(300);
        for _ in 0..300 {
            buffer.push_back(Default::default());
        }

        DisplayModule::new()
            .name(name)
            .input(IN_INPUT, "Input")
            .param(WET_PARAM, 0.0, 1.0, 0.2, "Wet", "", false)
            .param(TIME_PARAM, 0.0, 20.0, 5.0, "Time", " s", false)
            .output(OUT_OUTPUT, "Output")
            .start(Reverb { buffer })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Reverb {
    fn process(
        &mut self,
        input: &[AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        params: &[f32; NUM_PARAMS],
    ) {
        let mut feedback: AudioPacket = Default::default();
        for (input, output, buffer, fb) in izip!(
            input[IN_INPUT].data,
            output[OUT_OUTPUT].data.iter_mut(),
            self.buffer.pop_front().unwrap().data,
            feedback.data.iter_mut()
        ) {
            for (fin, fo, b, fbk) in izip!(
                input.data,
                output.data.iter_mut(),
                buffer.data,
                fb.data.iter_mut(),
            ) {
                *fo = (fin as f32 + b as f32 * 0.5).round() as i16;
                *fbk = (fin as f32 * 0.5 + b as f32 * 0.25).round() as i16;
            }
        }
        self.buffer.push_back(feedback);
    }
}
