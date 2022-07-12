use apiary_core::{
    socket_native::NativeInterface, voct_to_frequency, Module, BLOCK_SIZE, CHANNELS, SAMPLE_RATE,
};
use eframe::egui;
use std::{
    f32::consts::PI,
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack, UiUpdate};

pub struct Oscillator {
    width: f32,
    open: bool,
    tx: Sender<UiUpdate>,
    input_checked: bool,
    sin_checked: bool,
    tri_checked: bool,
    saw_checked: bool,
    sqr_checked: bool,
}

// const WT: usize = 2048;  // Wavetable size in samples

impl Oscillator {
    pub fn new() -> Self {
        let (ui_tx, ui_rx): (Sender<UiUpdate>, Receiver<UiUpdate>) = channel();

        thread::spawn(move || {
            let mut module: Module<_, _, 1, 4> = Module::new(
                NativeInterface::new().unwrap(),
                rand::thread_rng(),
                "Oscillator".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;
            let mut phase = [0.0; CHANNELS];

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
                    module
                        .poll(time, |input, output| {
                            for i in 0..BLOCK_SIZE {
                                for j in 0..CHANNELS {
                                    let a = 8000.0;
                                    output[0].data[i].data[j] =
                                        (a * (2.0 * PI * phase[j]).sin()).round() as i16;
                                    output[1].data[i].data[j] = if phase[j] < 0.5 {
                                        -a + 4.0 * a * phase[j]
                                    } else {
                                        a - 4.0 * a * (phase[j] - 0.5)
                                    }
                                    .round()
                                        as i16;
                                    output[2].data[i].data[j] =
                                        (-a + 2.0 * a * phase[j]).round() as i16;
                                    output[3].data[i].data[j] =
                                        if phase[j] < 0.5 { a } else { -a }.round() as i16;
                                    phase[j] +=
                                        voct_to_frequency(input[0].data[i].data[j]) / SAMPLE_RATE;
                                    while phase[j] > 1.0 {
                                        phase[j] -= 1.0;
                                    }
                                }
                            }
                        })
                        .unwrap();
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        Oscillator {
            width: 5.0,
            open: true,
            tx: ui_tx,
            input_checked: false,
            sin_checked: false,
            tri_checked: false,
            saw_checked: false,
            sqr_checked: false,
        }
    }
}

impl DisplayModule for Oscillator {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("Oscillator");
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
        if ui.add(Jack::new(&mut self.sin_checked, "Sin")).changed() {
            self.tx
                .send(UiUpdate {
                    input: false,
                    id: 0,
                    on: self.sin_checked,
                })
                .unwrap();
        }
        if ui.add(Jack::new(&mut self.tri_checked, "Tri")).changed() {
            self.tx
                .send(UiUpdate {
                    input: false,
                    id: 1,
                    on: self.tri_checked,
                })
                .unwrap();
        }
        if ui.add(Jack::new(&mut self.saw_checked, "Saw")).changed() {
            self.tx
                .send(UiUpdate {
                    input: false,
                    id: 2,
                    on: self.saw_checked,
                })
                .unwrap();
        }
        if ui.add(Jack::new(&mut self.sqr_checked, "Sqr")).changed() {
            self.tx
                .send(UiUpdate {
                    input: false,
                    id: 3,
                    on: self.sqr_checked,
                })
                .unwrap();
        }
    }
}
