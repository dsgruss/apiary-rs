use apiary_core::{softclip, voct_to_freq_scale, AudioPacket, BLOCK_SIZE, CHANNELS, SAMPLE_RATE};
use rand::Rng;
use std::f32::consts::PI;

use crate::display_module::{DisplayModule, Processor};

struct LadderFilter {
    omega0: f32,
    input: f32,
    state: [f32; 4],
    resonance: f32,
}

impl Default for LadderFilter {
    fn default() -> Self {
        LadderFilter {
            omega0: 2.0 * PI * 1000.0,
            input: 0.0,
            state: [0.0; 4],
            resonance: 1.0,
        }
    }
}

impl LadderFilter {
    fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.omega0 = 2.0 * PI * cutoff.clamp(0.0, 8000.0);
        self.resonance = resonance;
    }

    fn process(&mut self, input: f32, dt: f32) -> f32 {
        let mut state = self.state.clone();
        self.rk4(dt, &mut state, self.input / 16000.0, input / 16000.0);
        self.state = state;
        self.input = input;
        self.state[3] * 16000.0
    }

    fn f(&mut self, t: f32, x: [f32; 4], input: f32, input_new: f32, dt: f32) -> [f32; 4] {
        let mut dxdt = [0.0; 4];
        let inputt = input * (t / dt) + input_new * (1.0 - t / dt);
        let inputc = softclip(inputt - self.resonance * x[3]);
        let yc = x.map(softclip);

        dxdt[0] = self.omega0 * (inputc - yc[0]);
        dxdt[1] = self.omega0 * (yc[0] - yc[1]);
        dxdt[2] = self.omega0 * (yc[1] - yc[2]);
        dxdt[3] = self.omega0 * (yc[2] - yc[3]);
        dxdt
    }

    fn rk4(&mut self, dt: f32, x: &mut [f32; 4], input: f32, input_new: f32) {
        let mut yi = [0.0; 4];

        let k1 = self.f(0.0, *x, input, input_new, dt);
        for i in 0..4 {
            yi[i] = x[i] + k1[i] * dt / 2.0;
        }
        let k2 = self.f(dt / 2.0, yi, input, input_new, dt);
        for i in 0..4 {
            yi[i] = x[i] + k2[i] * dt / 2.0;
        }
        let k3 = self.f(dt / 2.0, yi, input, input_new, dt);
        for i in 0..4 {
            yi[i] = x[i] + k3[i] * dt;
        }
        let k4 = self.f(dt, yi, input, input_new, dt);
        for i in 0..4 {
            x[i] += dt * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]) / 6.0;
        }
    }
}

pub struct Filter {
    filters: [LadderFilter; CHANNELS],
}

const FREQ_PARAM: usize = 0;
const RES_PARAM: usize = 1;
const NUM_PARAMS: usize = 2;

const IN_INPUT: usize = 0;
const KEY_INPUT: usize = 1;
const NUM_INPUTS: usize = 2;

const LPF_OUTPUT: usize = 0;
const NUM_OUTPUTS: usize = 1;

impl Filter {
    pub fn init() -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
        DisplayModule::new()
            .name("Filter")
            .param(FREQ_PARAM, 20.0, 8000.0, 4000.0, "Cutoff", " Hz", true)
            .param(RES_PARAM, 0.0, 1.0, 0.0, "Resonance", "", false)
            .input(IN_INPUT, "Audio")
            .input(KEY_INPUT, "Key Track")
            .output(LPF_OUTPUT, "Lowpass Filter")
            .start(Filter {
                filters: Default::default(),
            })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for Filter {
    fn process(
        &mut self,
        input: &[AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        params: &[f32; NUM_PARAMS],
    ) {
        let mut rng = rand::thread_rng();
        for i in 0..BLOCK_SIZE {
            for j in 0..CHANNELS {
                self.filters[j].set_params(
                    params[FREQ_PARAM]
                        * voct_to_freq_scale(input[KEY_INPUT].data[i].data[j] as f32),
                    params[RES_PARAM].powi(2) * 10.0,
                );
                output[LPF_OUTPUT].data[i].data[j] = self.filters[j]
                    .process(
                        input[IN_INPUT].data[i].data[j] as f32 + rng.gen_range(-1e-6..1e-6),
                        1.0 / SAMPLE_RATE,
                    )
                    .round() as i16;
            }
        }
    }
}
