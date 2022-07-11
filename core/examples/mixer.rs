use apiary_core::{socket_native::NativeInterface, Module, BLOCK_SIZE, CHANNELS};
use eframe::egui;
use std::{
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack, UiUpdate};

pub struct Mixer {
    width: f32,
    open: bool,
    tx: Sender<UiUpdate>,
    input_checked: bool,
    gate_checked: bool,
    output_checked: bool,
}

impl Mixer {
    pub fn new() -> Self {
        let (ui_tx, ui_rx): (Sender<UiUpdate>, Receiver<UiUpdate>) = channel();

        thread::spawn(move || {
            let mut module: Module<_, _, 2, 1> = Module::new(
                NativeInterface::new(2, 1).unwrap(),
                rand::thread_rng(),
                "Mixer".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;

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
                                    output[0].data[i].data[j] = (input[0].data[i].data[j] as f32
                                        * (input[1].data[i].data[j] as f32 / i16::MAX as f32))
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

        Mixer {
            width: 5.0,
            open: true,
            tx: ui_tx,
            input_checked: false,
            gate_checked: false,
            output_checked: false,
        }
    }
}

impl DisplayModule for Mixer {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mixer");
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
        if ui.add(Jack::new(&mut self.gate_checked, "Gate")).changed() {
            self.tx
                .send(UiUpdate {
                    input: true,
                    id: 1,
                    on: self.gate_checked,
                })
                .unwrap();
        }
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
