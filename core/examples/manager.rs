use apiary_core::Module;
use eframe::egui;
use simple_logger::SimpleLogger;
use std::{
    sync::mpsc::{channel, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

mod common;
use common::SelectedInterface;

mod midi_to_cv;
use midi_to_cv::MidiToCv;

mod oscillator;
use oscillator::Oscillator;

mod mixer;
use mixer::Mixer;

mod filter;
use filter::Filter;

mod audio_interface;
use audio_interface::AudioInterface;

// mod oscilloscope;
// use oscilloscope::Oscilloscope;

mod display_module;
use display_module::DisplayHandler;

#[macro_use]
extern crate log;

fn main() {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .without_timestamps()
        .init()
        .unwrap();

    let grid_size = (36.0, 19.0);
    let grid_pos = (0.0, 0.0);

    let (tx, rx) = channel();

    thread::spawn(move || {
        let mut module: Module<_, _, 0, 0> = Module::new(
            SelectedInterface::new().unwrap(),
            rand::thread_rng(),
            "Manager".into(),
            0,
        );
        let start = Instant::now();
        let mut time: i64 = 0;

        'outer: loop {
            while time < start.elapsed().as_millis() as i64 {
                module.poll(time, |_, _| {}).unwrap();
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
    windows: Vec<(u32, Box<dyn DisplayHandler>)>,
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
                    if ui.button("Midi to CV").clicked() {
                        self.windows
                            .push((self.window_count, Box::new(MidiToCv::init())));
                        self.window_count += 1;
                    }
                    if ui.button("Oscillator").clicked() {
                        self.windows
                            .push((self.window_count, Box::new(Oscillator::init())));
                        self.window_count += 1;
                    }
                    if ui.button("Mixer").clicked() {
                        self.windows
                            .push((self.window_count, Box::new(Mixer::init())));
                        self.window_count += 1;
                    }
                    if ui.button("Filter").clicked() {
                        self.windows
                            .push((self.window_count, Box::new(Filter::init())));
                        self.window_count += 1;
                    }
                    if ui.button("Audio Interface").clicked() {
                        match AudioInterface::init() {
                            Ok(a) => {
                                self.windows.push((self.window_count, Box::new(a)));
                                self.window_count += 1;
                            }
                            Err(e) => info!("Failed to open AudioInterface: {:?}", e),
                        }
                    }
                    // if ui.button("Oscilloscope").clicked() {
                    //     self.windows
                    //         .push((self.window_count, Box::new(Oscilloscope::new())));
                    //     self.window_count += 1;
                    // }
                    ui.add_space(100.0);
                    ui.label(format!("{}", self.status));
                },
            );
        });
        self.windows.retain(|w| w.1.is_open());
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                for w in &mut self.windows {
                    egui::Area::new(format!("id{:?}", w.0))
                        // egui::containers::Resize::default()
                        //    .fixed_size((15.0 * w.width(), 450.0))
                        .show(ctx, |ui| {
                            ui.vertical(|ui| {
                                egui::containers::Frame::none()
                                    .rounding(2.0)
                                    .stroke((1.0, egui::Color32::BLACK).into())
                                    .inner_margin(4.0)
                                    .show(ui, |mut ui| {
                                        w.1.update(&mut ui);
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
