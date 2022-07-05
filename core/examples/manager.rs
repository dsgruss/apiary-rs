use apiary_core::{socket_native::NativeInterface, Directive, DirectiveHalt, Module, Uuid};
use eframe::egui::{self};
use simple_logger::SimpleLogger;
use std::{
    str::FromStr,
    sync::mpsc::{channel, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

mod common;
use common::DisplayModule;

mod midi_to_cv;
use midi_to_cv::MidiToCv;

#[macro_use]
extern crate log;

fn main() {
    SimpleLogger::new().init().unwrap();

    let grid_size = (36.0, 19.0);
    let grid_pos = (0.0, 0.0);

    let (tx, rx) = channel();

    let mut module = Module::new(NativeInterface::new(0, 0).unwrap());
    let start = Instant::now();
    let out = Directive::Halt(DirectiveHalt {
        uuid: Uuid::from_str("GLOBAL").unwrap(),
    });
    let mut time = 0;
    thread::spawn(move || loop {
        while time < start.elapsed().as_millis() {
            module.poll(start.elapsed().as_millis() as i64).unwrap();
            match rx.try_recv() {
                Ok(true) => module.send_directive(&out).unwrap(),
                Ok(false) => {}
                Err(TryRecvError::Empty) => {}
                Err(e) => panic!("{:?}", e),
            }
            time += 1;
        }
        thread::sleep(Duration::from_millis(0));
    });

    let mut options = eframe::NativeOptions::default();
    options.initial_window_size = Some([grid_size.0 * 50.0 - 10.0, grid_size.1 * 50.0].into());
    options.initial_window_pos = Some([grid_pos.0 * 50.0 + 15.0, grid_pos.1 * 50.0 + 10.0].into());
    options.resizable = false;
    eframe::run_native(
        "Module Test Sandbox",
        options,
        Box::new(|_cc| Box::new(Manager::new(tx))),
    );
}

struct Manager {
    status: String,
    tx: Sender<bool>,
    windows: Vec<Box<dyn DisplayModule>>,
    window_count: u32,
}

impl Manager {
    fn new(tx: Sender<bool>) -> Self {
        Self {
            status: "Loading...".to_owned(),
            tx,
            windows: vec![],
            window_count: 0,
        }
    }
}

impl eframe::App for Manager {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.heading("Manager");
            ui.add_space(20.0);
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    if ui.button("🔌    Close All").clicked() {
                        // Send halt directive
                        info!("Close button clicked");
                        self.tx.send(true).unwrap();
                    }
                    if ui.button("Save Preset").clicked() {
                        // Gather preset information
                    }
                    if ui.button("Load Preset").clicked() {
                        // Send preset information
                    }
                    ui.add_space(20.0);
                    if ui.button("Midi to CV").clicked() {
                        self.windows.push(Box::new(MidiToCv::new(format!(
                            "Test{}",
                            self.window_count
                        ))));
                        self.window_count += 1;
                    }
                    ui.add_space(100.0);
                    ui.label(format!("{}", self.status));
                },
            );
        });
        self.windows.retain(|w| w.is_open());
        for w in &mut self.windows {
            w.update(ctx);
        }
    }
}
