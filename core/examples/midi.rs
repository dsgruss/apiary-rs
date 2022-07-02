use apiary_core::{socket_native::NativeInterface, Module, Directive, DirectiveHalt, Uuid};
use eframe::egui;
use simple_logger::SimpleLogger;
use std::{time::{Instant, Duration}, thread, str::FromStr};

fn main() {
    SimpleLogger::new().init().unwrap();
    let mut module = Module::new(NativeInterface::new());
    let start = Instant::now();
    let out = Directive::Halt(DirectiveHalt { uuid: Uuid::from_str("GLOBAL").unwrap()} );
    let mut time = 0;
    loop {
        while time < start.elapsed().as_millis() {
            module.poll(start.elapsed().as_millis() as i64).unwrap();
            module.send_directive(&out).unwrap();
            time += 1;
        }
        thread::sleep(Duration::from_millis(0));
    }

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    );
}

struct MyApp {
    name: String,
    age: u32,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            name: "Arthur".to_owned(),
            age: 42,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("My egui Application");
            ui.horizontal(|ui| {
                ui.label("Your name: ");
                ui.text_edit_singleline(&mut self.name);
            });
            ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
            if ui.button("Click each year").clicked() {
                self.age += 1;
            }
            ui.label(format!("Hello '{}', age {}", self.name, self.age));
        });
    }
}