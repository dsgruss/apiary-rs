use apiary_core::{socket_native::NativeInterface, Module};
use eframe::egui;
use std::{
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::DisplayModule;

pub struct MODULENAME {
    name: String,
    open: bool,
    tx: Sender<()>,
}

impl MODULENAME {
    pub fn new(name: String) -> Self {

        let (ui_tx, ui_rx): (Sender<()>, Receiver<()>) = channel();

        thread::spawn(move || {
            let mut module = Module::new(
                NativeInterface::new(0, 3).unwrap(),
                rand::thread_rng(),
                "MODULENAME".into(),
                0,
            );
            let start = Instant::now();
            let mut time = 0;

            'outer: loop {
                while time < start.elapsed().as_millis() {
                    match ui_rx.try_recv() {
                        Ok(message) => {
                            // Add message handling from ui
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }
                    module.poll(start.elapsed().as_millis() as i64).unwrap();
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        MODULENAME {
            name,
            open: true,
            tx: ui_tx,
        }
    }
}

impl DisplayModule for MODULENAME {
    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ctx: &egui::Context) {
        egui::Window::new(&self.name)
            .open(&mut self.open)
            .collapsible(false)
            .resizable(false)
            .min_height(450.0)
            .min_width(190.0)
            .show(ctx, |ui| {
                ui.heading("MODULENAME");
                ui.add_space(20.0);
                // Add ui and message transmission
                ui.allocate_space(ui.available_size());
            });
    }
}
