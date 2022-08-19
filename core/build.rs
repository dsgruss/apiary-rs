//! This build script creates the wavetable files during compile time, since they end up being a
//! chunk of static memory embedded in the executable.

use fixed::types::I1F15;
use rustfft::{num_complex::Complex, FftPlanner};
use std::fs::File;
use std::io::Write;
use std::{f32::consts::PI, mem};
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

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct WavetableFP {
    vals: [[I1F15; 2048]; 9],
}

impl WavetableFP {
    fn new(wt: Wavetable) -> Self {
        let mut vals: [[I1F15; 2048]; 9] = [[I1F15::from_num(0.0); 2048]; 9];
        for i in 0..2048 {
            for j in 0..9 {
                vals[j][i] = I1F15::from_num(wt.vals[j][i]);
            }
        }
        WavetableFP { vals }
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

fn write_wavetable(wt: Wavetable, csv: &str, header: &str, fval: &str, fpval: &str) {
    let mut f = File::create(csv).unwrap();
    writeln!(f, "{}", header).unwrap();
    for i in 0..2048 {
        for j in 0..9 {
            write!(f, "{}, ", wt.vals[j][i]).unwrap();
        }
        write!(f, "\n").unwrap();
    }
    File::create(fval)
        .unwrap()
        .write_all(wt.as_bytes())
        .unwrap();
    let wtfp = &WavetableFP::new(wt);
    let mut f_fp = File::create(fpval).unwrap();
    unsafe {
        f_fp.write_all(
            &*(wtfp as *const WavetableFP as *const [u8; mem::size_of::<WavetableFP>()]),
        )
        .unwrap();
    }
}

fn main() {
    let mut sin = [0.0; 2048];
    for i in 0..2048 {
        sin[i] = (i as f32 * 2.0 * PI / 2048.0).sin();
    }
    write_wavetable(
        generate_wavetable(sin),
        "wt/sin.csv",
        "# Sine function wavetable",
        "wt/sin.f32",
        "wt/sin.i1f15",
    );

    let mut tri = [0.0; 2048];
    for i in 0..2048 {
        let phase = i as f32 / 2048.0;
        tri[i] = if phase < 0.5 {
            -1.0 + 4.0 * phase
        } else {
            1.0 - 4.0 * (phase - 0.5)
        };
    }
    write_wavetable(
        generate_wavetable(tri),
        "wt/tri.csv",
        "# Triangle function wavetable",
        "wt/tri.f32",
        "wt/tri.i1f15",
    );

    let mut saw = [0.0; 2048];
    for i in 0..2048 {
        saw[i] = -1.0 + 2.0 * (i as f32) / 2048.0;
    }
    write_wavetable(
        generate_wavetable(saw),
        "wt/saw.csv",
        "# Sawtooth function wavetable",
        "wt/saw.f32",
        "wt/saw.i1f15",
    );

    let mut sqr = [0.0; 2048];
    for i in 0..2048 {
        sqr[i] = if i < 1024 { -1.0 } else { 1.0 };
    }
    write_wavetable(
        generate_wavetable(sqr),
        "wt/sqr.csv",
        "# Square wave wavetable",
        "wt/sqr.f32",
        "wt/sqr.i1f15",
    );
}
