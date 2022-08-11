use apiary_core::{
    AudioPacket, InputJackHandle, Module, Network, OutputJackHandle, PollUpdate, ProcessBlock,
    SampleType, BLOCK_SIZE, CHANNELS, SAMPLE_RATE,
};
use libm::{log10f, powf};
use palette::Srgb;
use rand_core::RngCore;
use stm32f4xx_hal::gpio;

use crate::ui::Switch;

pub const NUM_INPUTS: usize = 1;
pub const NUM_OUTPUTS: usize = 1;
pub const COLOR: u16 = 0;
pub const NAME: &str = "envelope";

const DELAY_PARAM: usize = 0;
const ATTACK_PARAM: usize = 1;
const HOLD_PARAM: usize = 2;
const DECAY_PARAM: usize = 3;
const SUSTAIN_PARAM: usize = 4;
const RELEASE_PARAM: usize = 5;
const NUM_PARAMS: usize = 6;

#[derive(Copy, Clone, PartialEq)]
enum Stage {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

pub struct EnvelopePins {
    pub gate: gpio::Pin<'D', 12>,
    pub level: gpio::Pin<'D', 13>,
}

pub struct Envelope {
    gate: Switch<'D', 12>,
    level_sw: Switch<'D', 13>,
    jack_gate: InputJackHandle,
    jack_level: OutputJackHandle,
    params: [f32; NUM_PARAMS],
    stage: [Stage; CHANNELS],
    frame_counter: i64,
    start: [i64; CHANNELS],
    level: [f32; CHANNELS],
}

impl Envelope {
    pub fn new<T, R>(pins: EnvelopePins, module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>) -> Self
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        Envelope {
            gate: Switch::new(pins.gate),
            level_sw: Switch::new(pins.level),
            jack_gate: module.add_input_jack().unwrap(),
            jack_level: module.add_output_jack().unwrap(),
            params: [0.0; NUM_PARAMS],
            stage: [Stage::Release; CHANNELS],
            frame_counter: 0,
            start: [0; CHANNELS],
            level: [0.0; CHANNELS],
        }
    }

    pub fn poll_ui<T, R>(&mut self, module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>)
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        self.gate.debounce();
        self.level_sw.debounce();

        if self.gate.changed() || self.level_sw.changed() {
            module
                .set_input_patch_enabled(self.jack_gate, self.gate.just_pressed())
                .unwrap();
            module
                .set_output_patch_enabled(self.jack_level, self.level_sw.just_pressed())
                .unwrap();
        }
    }

    pub fn process(&mut self, block: &mut ProcessBlock<NUM_INPUTS, NUM_OUTPUTS>) {
        let mut output = AudioPacket::default();
        let input = block.get_input(self.jack_gate);
        let dt = 1.0 / SAMPLE_RATE;
        for i in 0..BLOCK_SIZE {
            self.frame_counter += 1;
            for j in 0..CHANNELS {
                if input.data[i].data[j] < 1024 {
                    self.stage[j] = Stage::Release;
                    let step = self.params[SUSTAIN_PARAM] / self.params[RELEASE_PARAM] * dt;
                    self.level[j] = (self.level[j] - step).clamp(0.0, 1.0);
                } else {
                    if self.stage[j] == Stage::Release {
                        self.stage[j] = Stage::Delay;
                        self.start[j] = self.frame_counter;
                    }
                    if self.stage[j] == Stage::Delay {
                        if self.frame_counter
                            >= self.start[j] + (self.params[DELAY_PARAM] / dt) as i64
                        {
                            self.stage[j] = Stage::Attack;
                            self.start[j] = self.frame_counter;
                        } else {
                            self.level[j] = 0.0;
                        }
                    }
                    if self.stage[j] == Stage::Attack {
                        if self.frame_counter
                            >= self.start[j] + (self.params[ATTACK_PARAM] / dt) as i64
                        {
                            self.stage[j] = Stage::Hold;
                            self.start[j] = self.frame_counter;
                        } else {
                            let step = 1.0 / self.params[ATTACK_PARAM] * dt;
                            self.level[j] = (self.level[j] + step).clamp(0.0, 1.0);
                        }
                    }
                    if self.stage[j] == Stage::Hold {
                        if self.frame_counter
                            >= self.start[j] + (self.params[HOLD_PARAM] / dt) as i64
                        {
                            self.stage[j] = Stage::Decay;
                            self.start[j] = self.frame_counter;
                        } else {
                            self.level[j] = 1.0;
                        }
                    }
                    if self.stage[j] == Stage::Decay {
                        if self.frame_counter
                            >= self.start[j] + (self.params[DECAY_PARAM] / dt) as i64
                        {
                            self.stage[j] = Stage::Sustain;
                            self.start[j] = self.frame_counter;
                        } else {
                            let step =
                                (1.0 - self.params[SUSTAIN_PARAM]) / self.params[DECAY_PARAM] * dt;
                            self.level[j] =
                                (self.level[j] - step).clamp(self.params[SUSTAIN_PARAM], 1.0);
                        }
                    }
                    if self.stage[j] == Stage::Sustain {
                        self.level[j] = self.params[SUSTAIN_PARAM];
                    }
                }
                output.data[i].data[j] =
                    (self.level[j] * SampleType::MAX as f32 * 0.9) as SampleType;
            }
        }
        block.set_output(self.jack_level, output);
    }

    pub fn set_params(&mut self, adc: &mut [u16; 8]) {
        self.params[DELAY_PARAM] = 0.0;
        self.params[ATTACK_PARAM] += 0.01
            * (0.01 * powf(10.0, (adc[0] as f32 / 4096.0) * log10f(20.0 / 0.01))
                - self.params[ATTACK_PARAM]);
        self.params[HOLD_PARAM] = 0.0;
        self.params[DECAY_PARAM] += 0.01
            * (0.01 * powf(10.0, (adc[1] as f32 / 4096.0) * log10f(20.0 / 0.01))
                - self.params[DECAY_PARAM]);
        self.params[SUSTAIN_PARAM] += 0.01 * (adc[2] as f32 / 4096.0 - self.params[SUSTAIN_PARAM]);
        self.params[RELEASE_PARAM] += 0.01
            * (0.01 * powf(10.0, (adc[3] as f32 / 4096.0) * log10f(20.0 / 0.01))
                - self.params[RELEASE_PARAM]);
    }

    pub fn get_light_data(&self, update: PollUpdate<NUM_INPUTS, NUM_OUTPUTS>) -> [Srgb<u8>; 2] {
        [
            update.get_input_color(self.jack_gate),
            update.get_output_color(self.jack_level),
        ]
    }
}
