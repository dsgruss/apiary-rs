use apiary_core::{AudioPacket, SampleType, BLOCK_SIZE, CHANNELS, SAMPLE_RATE};

use crate::display_module::{DisplayModule, Processor};

pub struct Envelope {
    stage: [Stage; CHANNELS],
    frame_counter: i64,
    start: [i64; CHANNELS],
    level: [f32; CHANNELS],
}

const DELAY_PARAM: usize = 0;
const ATTACK_PARAM: usize = 1;
const HOLD_PARAM: usize = 2;
const DECAY_PARAM: usize = 3;
const SUSTAIN_PARAM: usize = 4;
const RELEASE_PARAM: usize = 5;
const NUM_PARAMS: usize = 6;

const GATE_INPUT: usize = 0;
const NUM_INPUTS: usize = 1;

const LEVEL_OUTPUT: usize = 0;
const NUM_OUTPUTS: usize = 1;

#[derive(Copy, Clone, PartialEq)]
enum Stage {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

impl Envelope {
    pub fn init(name: &str) -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name(name)
            .input(GATE_INPUT, "Gate")
            .param(DELAY_PARAM, 0.001, 4.0, 0.0, "Delay", " s", true)
            .param(ATTACK_PARAM, 0.001, 20.0, 0.5, "Attack", " s", true)
            .param(HOLD_PARAM, 0.001, 4.0, 0.0, "Hold", " s", true)
            .param(DECAY_PARAM, 0.001, 20.0, 0.5, "Decay", " s", true)
            .param(SUSTAIN_PARAM, 0.0, 1.0, 0.5, "Sustain", "", false)
            .param(RELEASE_PARAM, 0.001, 20.0, 0.5, "Release", " s", true)
            .output(LEVEL_OUTPUT, "Level")
            .start(Envelope {
                stage: [Stage::Release; CHANNELS],
                frame_counter: 0,
                start: [0; CHANNELS],
                level: [0.0; CHANNELS],
            })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Envelope {
    fn process(
        &mut self,
        input: [&AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        params: &[f32; NUM_PARAMS],
    ) {
        let dt = 1.0 / SAMPLE_RATE;
        for i in 0..BLOCK_SIZE {
            if self.frame_counter % 10000 == 0 {
                trace!("{:?} {:?} {:?}", dt, self.level, params)
            }
            self.frame_counter += 1;
            for j in 0..CHANNELS {
                if input[GATE_INPUT].data[i].data[j] < 1024 {
                    self.stage[j] = Stage::Release;
                    let step = params[SUSTAIN_PARAM] / params[RELEASE_PARAM] * dt;
                    self.level[j] = (self.level[j] - step).clamp(0.0, 1.0);
                } else {
                    if self.stage[j] == Stage::Release {
                        self.stage[j] = Stage::Delay;
                        self.start[j] = self.frame_counter;
                    }
                    if self.stage[j] == Stage::Delay {
                        if self.frame_counter
                            >= self.start[j] + (params[DELAY_PARAM] / dt).round() as i64
                        {
                            self.stage[j] = Stage::Attack;
                            self.start[j] = self.frame_counter;
                        } else {
                            self.level[j] = 0.0;
                        }
                    }
                    if self.stage[j] == Stage::Attack {
                        if self.frame_counter
                            >= self.start[j] + (params[ATTACK_PARAM] / dt).round() as i64
                        {
                            self.stage[j] = Stage::Hold;
                            self.start[j] = self.frame_counter;
                        } else {
                            let step = 1.0 / params[ATTACK_PARAM] * dt;
                            self.level[j] = (self.level[j] + step).clamp(0.0, 1.0);
                        }
                    }
                    if self.stage[j] == Stage::Hold {
                        if self.frame_counter
                            >= self.start[j] + (params[HOLD_PARAM] / dt).round() as i64
                        {
                            self.stage[j] = Stage::Decay;
                            self.start[j] = self.frame_counter;
                        } else {
                            self.level[j] = 1.0;
                        }
                    }
                    if self.stage[j] == Stage::Decay {
                        if self.frame_counter
                            >= self.start[j] + (params[DECAY_PARAM] / dt).round() as i64
                        {
                            self.stage[j] = Stage::Sustain;
                            self.start[j] = self.frame_counter;
                        } else {
                            let step = (1.0 - params[SUSTAIN_PARAM]) / params[DECAY_PARAM] * dt;
                            self.level[j] =
                                (self.level[j] - step).clamp(params[SUSTAIN_PARAM], 1.0);
                        }
                    }
                    if self.stage[j] == Stage::Sustain {
                        self.level[j] = params[SUSTAIN_PARAM];
                    }
                }
                output[LEVEL_OUTPUT].data[i].data[j] =
                    (self.level[j] * SampleType::MAX as f32 * 0.9).round() as SampleType;
            }
        }
    }
}
