use apiary_core::{socket_native::NativeInterface, Module};
use eframe::egui;
use std::{
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::DisplayModule;

pub struct MODULENAME {
    width: f32,
    open: bool,
    tx: Sender<()>,
}

impl MODULENAME {
    pub fn new() -> Self {

        let (ui_tx, ui_rx): (Sender<()>, Receiver<()>) = channel();

        thread::spawn(move || {
            let mut module: Module<_, _, 0, 3> = Module::new(
                NativeInterface::new(0, 3).unwrap(),
                rand::thread_rng(),
                "MODULENAME".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;

            'outer: loop {
                while time < start.elapsed().as_millis() as i64 {
                    match ui_rx.try_recv() {
                        Ok(message) => {
                            // Add message handling from ui
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }
                    module.poll(time, |_, _| {}).unwrap();
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        MODULENAME {
            width: 5.0,
            open: true,
            tx: ui_tx,
        }
    }
}

impl DisplayModule for MODULENAME {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("MODULENAME");
        ui.add_space(20.0);
        // Add ui and message transmission
        ui.allocate_space(ui.available_size());
    }
}