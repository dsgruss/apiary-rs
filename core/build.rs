//! This build script creates the wavetable files during compile time, since they end up being a
//! chunk of static memory embedded in the executable.

use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;
use std::fs::File;
use std::io::Write;
use zerocopy::{AsBytes, FromBytes};

#[derive(AsBytes, FromBytes, Copy, Clone, Debug)]
#[repr(C)]
struct Wavetable {
    vals: [[f32; 2048]; 9],
}

impl Default for Wavetable {
    fn default() -> Self {
        Wavetable {
            vals: [[0.0; 2048]; 9],
        }
    }
}

fn generate_wavetable(input: [f32; 2048]) -> Wavetable {
    let mut result: Wavetable = Default::default();
    let mut wt = vec![
        Complex {
            re: 0.0_f32,
            im: 0.0_f32
        };
        2048
    ];
    for (i, x) in input.iter().enumerate() {
        wt[i].re = *x;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(2048);
    fft.process(&mut wt);

    planner = FftPlanner::<f32>::new();
    let rfft = planner.plan_fft_inverse(2048);
    for j in 0..9 {
        let mut wt_bl = vec![
            Complex {
                re: 0.0_f32,
                im: 0.0_f32
            };
            2048
        ];
        for (i, iwt) in wt.iter().enumerate() {
            let idx = 2_usize.pow(j) * i;
            if idx > 368 {
                break;
            }
            wt_bl[i] = *iwt;
        }
        rfft.process(&mut wt_bl);
        for (i, v) in wt_bl.iter().enumerate() {
            result.vals[j as usize][i] = v.re / 2048.0;
        }
    }
    result
}

fn main() {
    let mut sin = [0.0; 2048];
    for i in 0..2048 {
        sin[i] = (i as f32 * 2.0 * PI / 2048.0).sin();
    }
    File::create("wt/sin.in")
        .unwrap()
        .write_all(generate_wavetable(sin).as_bytes())
        .unwrap();

    let mut tri = [0.0; 2048];
    for i in 0..2048 {
        let phase = i as f32 / 2048.0;
        tri[i] = if phase < 0.5 {
            -1.0 + 4.0 * phase
        } else {
            1.0 - 4.0 * (phase - 0.5)
        };
    }
    File::create("wt/tri.in")
        .unwrap()
        .write_all(generate_wavetable(tri).as_bytes())
        .unwrap();

    let mut saw = [0.0; 2048];
    for i in 0..2048 {
        saw[i] = -1.0 + 2.0 * (i as f32) / 2048.0;
    }
    File::create("wt/saw.in")
        .unwrap()
        .write_all(generate_wavetable(saw).as_bytes())
        .unwrap();

    let mut sqr = [0.0; 2048];
    for i in 0..2048 {
        sqr[i] = if i < 1024 { -1.0 } else { 1.0 };
    }
    File::create("wt/sqr.in")
        .unwrap()
        .write_all(generate_wavetable(sqr).as_bytes())
        .unwrap();
}
