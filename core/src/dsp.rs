use core::f32::consts::PI;

use crate::softclip;

pub struct LadderFilter {
    omega0: f32,
    state: [f32; 4],
    resonance: f32,
    input: f32,
}

impl Default for LadderFilter {
    fn default() -> Self {
        LadderFilter {
            omega0: 2.0 * PI * 1000.0,
            state: [0.0; 4],
            resonance: 1.0,
            input: 0.0,
        }
    }
}

impl LadderFilter {
    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.omega0 = 2.0 * PI * cutoff.clamp(0.0, 8000.0);
        self.resonance = resonance;
    }

    pub fn process(&mut self, input: f32, dt: f32) -> f32 {
        let mut state = self.state.clone();
        self.rk4(dt, &mut state, self.input, input);
        self.state = state;
        self.input = input;
        self.state[3]
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
