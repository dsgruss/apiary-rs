use apiary_core::Module;
use eframe::egui;
use simple_logger::SimpleLogger;
use std::{
    sync::mpsc::{channel, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

#[macro_use]
extern crate lazy_static;

mod audio_interface;
mod common;
mod display_module;
mod envelope;
mod filter;
mod midi_to_cv;
mod mixer;
mod oscillator;
mod oscilloscope;
mod reverb;

use audio_interface::AudioInterface;
use common::SelectedInterface;
use display_module::DisplayHandler;
use envelope::Envelope;
use filter::Filter;
use midi_to_cv::MidiToCv;
use mixer::Mixer;
use oscillator::Oscillator;
use oscilloscope::Oscilloscope;
use reverb::Reverb;

fn window_build(name: &str, num: u32) -> Result<Box<dyn DisplayHandler>, ()> {
    let id = format!("{}:{}", name, num);
    match name {
        "Midi to CV" => Ok(Box::new(MidiToCv::init())),
        "Oscillator" => Ok(Box::new(Oscillator::init(&id))),
        "Envelope" => Ok(Box::new(Envelope::init(&id))),
        "Mixer" => Ok(Box::new(Mixer::init(&id))),
        "Filter" => Ok(Box::new(Filter::init())),
        "Audio Interface" => match AudioInterface::init() {
            Ok(a) => Ok(Box::new(a)),
            Err(e) => {
                info!("Failed to open AudioInterface: {:?}", e);
                Err(())
            }
        },
        "Reverb" => Ok(Box::new(Reverb::init(&id))),
        "Oscilloscope" => Ok(Box::new(Oscilloscope::new())),
        _ => Err(()),
    }
}

const WINDOWS: [&str; 8] = [
    "Midi to CV",
    "Oscillator",
    "Envelope",
    "Mixer",
    "Filter",
    "Audio Interface",
    "Reverb",
    "Oscilloscope",
];

#[macro_use]
extern crate log;

fn main() {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .without_timestamps()
        .init()
        .unwrap();

    let (tx, rx) = channel();

    thread::spawn(move || {
        let mut module: Module<_, _, 0, 0> = Module::new(
            SelectedInterface::new().unwrap(),
            rand::thread_rng(),
            "Manager".into(),
            0,
            0,
        );
        let start = Instant::now();
        let mut time: i64 = 0;

        'outer: loop {
            while time < start.elapsed().as_millis() as i64 {
                module.poll(time, |_| {}).unwrap();
                match rx.try_recv() {
                    Ok(true) => module.send_halt(),
                    Ok(false) => {}
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => break 'outer,
                }
                time += 1;
            }
            thread::sleep(Duration::from_millis(0));
        }
    });

    let options = eframe::NativeOptions {
        initial_window_size: Some([1790.0, 950.0].into()),
        initial_window_pos: Some([15.0, 10.0].into()),
        resizable: false,
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Module Test Sandbox",
        options,
        Box::new(|_cc| Box::new(Manager::new(tx))),
    );
}

struct Manager {
    status: String,
    tx: Sender<bool>,
    windows: Vec<Box<dyn DisplayHandler>>,
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
                        self.windows.clear();
                    }
                    if ui.button("Save Preset").clicked() {
                        // Gather preset information
                    }
                    if ui.button("Load Preset").clicked() {
                        // Send preset information
                    }
                    ui.add_space(20.0);
                    for w in WINDOWS {
                        if ui.button(w).clicked() {
                            match window_build(w, self.window_count) {
                                Ok(a) => {
                                    self.windows.push(a);
                                    self.window_count += 1;
                                }
                                Err(_) => {}
                            }
                        }
                    }
                    ui.add_space(100.0);
                    ui.label(format!("{}", self.status));
                },
            );
        });
        self.windows.retain(|w| w.is_open());
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                for w in &mut self.windows {
                    egui::Area::new(w.name())
                        // egui::containers::Resize::default()
                        //    .fixed_size((15.0 * w.width(), 450.0))
                        .show(ctx, |ui| {
                            ui.vertical(|ui| {
                                egui::containers::Frame::none()
                                    .rounding(2.0)
                                    .stroke((1.0, egui::Color32::BLACK).into())
                                    .inner_margin(4.0)
                                    .show(ui, |mut ui| {
                                        w.update(&mut ui);
                                        // ui.allocate_space(ui.available_size());
                                    });
                            });
                        });
                }
                ui.allocate_space(ui.available_size());
            });
        });
        ctx.request_repaint();
    }
}
