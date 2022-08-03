use core::{f32::consts::PI, cmp::min};

use libm::{sinf, roundf, floorf, ceilf};
use zerocopy::{AsBytes, FromBytes};

use crate::{voct_to_frequency, SAMPLE_RATE};

#[derive(Copy, Clone, Default)]
pub struct NaiveOscillator {
    level: f32,
    phase: f32,
}

impl NaiveOscillator {
    pub fn process(&mut self, note: i16, level: i16, prange: f32, plevel: f32) -> (i16, i16, i16, i16) {
        self.level += 0.01 * (level as f32 - self.level);

        let a = self.level * plevel;

        let sin = roundf(a * sinf(2.0 * PI * self.phase)) as i16;
        let tri = roundf(if self.phase < 0.5 {
            a * (-1.0 + 4.0 * self.phase)
        } else {
            a * (1.0 - 4.0 * (self.phase - 0.5))
        }
     ) as i16;
        let saw = roundf(-a + 2.0 * a * self.phase) as i16;
        let sqr = roundf(if self.phase < 0.5 { a } else { -a }) as i16;

        self.phase += voct_to_frequency(note as f32 + prange * 512.0) / SAMPLE_RATE;
        while self.phase > 1.0 {
            self.phase -= 1.0;
        }

        (sin, tri, saw, sqr)
    }
}

#[derive(Copy, Clone, Default)]
pub struct HarmOscillator {
    level: f32,
    phase: f32,
}

impl HarmOscillator {
    pub fn process(&mut self, note: i16, level: i16, prange: f32, plevel: f32) -> (i16, i16, i16, i16) {
        self.level += 0.01 * (level as f32 - self.level);

        let a = self.level * plevel;
        let freq = voct_to_frequency(note as f32 + prange * 512.0);
        let sin = a * sinf(2.0 * PI * self.phase);
        let mut tri = 0.0;
        let mut saw = 0.5;
        let mut sqr = 0.0;
        let nend = min(floorf(SAMPLE_RATE / (2.0 * freq)) as u32, 100);
        for i in 1..nend {
            let n = i as f32;
            if i % 2 != 0 {
                if ((i - 1) / 2) % 2 == 0 {
                    tri += a * 8.0 / (PI * PI * n * n) * sinf(n * 2.0 * PI * self.phase);
                } else {
                    tri -= a * 8.0 / (PI * PI * n * n) * sinf(n * 2.0 * PI * self.phase);
                }
                sqr += a * 4.0 / (PI * n) * sinf(n * 2.0 * PI * self.phase);
            }
            saw -= a / (PI * n) * sinf(n * 2.0 * PI * self.phase);
        }

        self.phase += freq / SAMPLE_RATE;
        while self.phase > 1.0 {
            self.phase -= 1.0;
        }

        (
            roundf(sin) as i16,
            roundf(tri) as i16,
            roundf(saw) as i16,
            roundf(sqr) as i16,
        )
    }
}

// https://www.earlevel.com/main/2012/05/09/a-wavetable-oscillator-part-3/

#[derive(Copy, Clone, Debug)]
pub struct WtOscillator {
    level: f32,
    phase: f32,
}

lazy_static! {
    static ref WTSIN: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../../wt/sin.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
    static ref WTTRI: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../../wt/tri.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
    static ref WTSAW: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../../wt/saw.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
    static ref WTSQR: [[f32; 2048]; 9] = {
        let bytes = include_bytes!("../../wt/sqr.in");
        Wavetable::read_from(&bytes[..]).unwrap().vals
    };
}

#[derive(AsBytes, FromBytes, Copy, Clone, Debug)]
#[repr(C)]
struct Wavetable {
    vals: [[f32; 2048]; 9],
}

impl Default for WtOscillator {
    fn default() -> Self {
        WtOscillator {
            level: 0.0,
            phase: 0.0,
        }
    }
}

impl WtOscillator {
    pub fn process(&mut self, note: i16, level: i16, prange: f32, plevel: f32) -> (i16, i16, i16, i16) {
        self.level += 0.01 * (level as f32 - self.level);

        let a = self.level * plevel;
        let freq = voct_to_frequency(note as f32 + prange * 512.0);

        let idx = match freq {
            f if f < 40.0 => 0,
            f if f < 80.0 => 0,
            f if f < 160.0 => 1,
            f if f < 320.0 => 2,
            f if f < 640.0 => 3,
            f if f < 1280.0 => 4,
            f if f < 2560.0 => 5,
            f if f < 5120.0 => 6,
            f if f < 10240.0 => 7,
            _ => 8,
        };

        let left = floorf(self.phase * 2048.0) as usize;
        let right = ceilf(self.phase * 2048.0) as usize % 2048;
        let frac = (self.phase * 2048.0) - floorf(self.phase * 2048.0);

        let sin = a * ((*WTSIN)[idx][left] * (1.0 - frac) + (*WTSIN)[idx][right] * frac);
        let tri = a * ((*WTTRI)[idx][left] * (1.0 - frac) + (*WTTRI)[idx][right] * frac);
        let saw = a * ((*WTSAW)[idx][left] * (1.0 - frac) + (*WTSAW)[idx][right] * frac);
        let sqr = a * ((*WTSQR)[idx][left] * (1.0 - frac) + (*WTSQR)[idx][right] * frac);

        self.phase += freq / SAMPLE_RATE;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        (
            roundf(sin) as i16,
            roundf(tri) as i16,
            roundf(saw) as i16,
            roundf(sqr) as i16,
        )
    }
}
