use apiary_core::{socket_native::NativeInterface, Module, BLOCK_SIZE, CHANNELS, SAMPLE_RATE};
use eframe::egui;
use std::{
    f32::consts::PI,
    sync::{
        mpsc::{channel, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack, Knob, UiUpdate};

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
        self.omega0 = 2.0 * PI * cutoff;
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
        let inputc = clip(inputt - self.resonance * x[3]);
        let yc = x.map(clip);

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

fn clip(x: f32) -> f32 {
    let x0 = if x < -3.0 {
        -3.0
    } else if x > 3.0 {
        3.0
    } else {
        x
    };
    x0 * (27.0 + x0 * x0) / (27.0 + 9.0 * x0 * x0)
}

pub struct Filter {
    width: f32,
    open: bool,
    tx: Sender<UiUpdate>,
    input_checked: bool,
    output_checked: bool,
    cutoff: Arc<Mutex<f32>>,
}

impl Filter {
    pub fn new() -> Self {
        let (ui_tx, ui_rx): (Sender<UiUpdate>, Receiver<UiUpdate>) = channel();
        let cutoff = Arc::new(Mutex::new(1000.0));
        let thread_cutoff = cutoff.clone();

        thread::spawn(move || {
            let mut module: Module<_, _, 1, 1> = Module::new(
                NativeInterface::new().unwrap(),
                rand::thread_rng(),
                "Filter".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;
            let mut filters: [LadderFilter; CHANNELS] = Default::default();

            'outer: loop {
                while time < start.elapsed().as_millis() as i64 {
                    match ui_rx.try_recv() {
                        Ok(msg) => {
                            if msg.input {
                                if let Err(e) = module.set_input_patch_enabled(msg.id, msg.on) {
                                    info!("Error {:?}", e);
                                }
                            } else {
                                if let Err(e) = module.set_output_patch_enabled(msg.id, msg.on) {
                                    info!("Error {:?}", e);
                                }
                            }
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }
                    let data = *thread_cutoff.lock().unwrap();
                    module
                        .poll(time, |input, output| {
                            for i in 0..BLOCK_SIZE {
                                for j in 0..CHANNELS {
                                    filters[j].set_params(data, 0.0);
                                    output[0].data[i].data[j] = filters[j]
                                        .process(input[0].data[i].data[j] as f32, 1.0 / SAMPLE_RATE)
                                        .round()
                                        as i16;
                                }
                            }
                        })
                        .unwrap();
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        Filter {
            width: 5.0,
            open: true,
            tx: ui_tx,
            input_checked: false,
            output_checked: false,
            cutoff,
        }
    }
}

impl DisplayModule for Filter {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("Filter");
        ui.add_space(20.0);
        if ui
            .add(Jack::new(&mut self.input_checked, "Input"))
            .changed()
        {
            self.tx
                .send(UiUpdate {
                    input: true,
                    id: 0,
                    on: self.input_checked,
                })
                .unwrap();
        }
        ui.add_space(20.0);
        let mut data = self.cutoff.lock().unwrap();
        ui.add(Knob::new(&mut data, "Cutoff", 20.0, 8000.0, true));
        ui.add_space(20.0);
        if ui
            .add(Jack::new(&mut self.output_checked, "Output"))
            .changed()
        {
            self.tx
                .send(UiUpdate {
                    input: false,
                    id: 0,
                    on: self.output_checked,
                })
                .unwrap();
        }
    }
}
