use apiary_core::{socket_native::NativeInterface, Module};
use eframe::egui;
use midir::{MidiInput, MidiInputConnection};
use std::{
    sync::mpsc::{channel, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::DisplayModule;

pub struct MidiToCv {
    name: String,
    open: bool,
    _midi_connections: Vec<MidiInputConnection<()>>,
    note_checked: bool,
    gate_checked: bool,
    mdwh_checked: bool,
}

#[derive(Debug)]
enum MidiMessage {
    NoteOn(u8, u8),
    NoteOff(u8, u8),
    Unimplemented,
}

impl MidiToCv {
    pub fn new(name: String) -> Self {
        let (tx, rx) = channel();

        let midi_check = MidiInput::new("midir port enumerator").unwrap();
        info!("Opening all MIDI inputs by default");
        let in_ports = midi_check.ports();
        let mut midi_connections = vec![];
        for (i, p) in in_ports.iter().enumerate() {
            info!("Opening {:?}", midi_check.port_name(p).unwrap());
            let midi_in = MidiInput::new(&format!("midir input {}", i)).unwrap();
            let port_tx = tx.clone();
            let conn_in = midi_in
                .connect(
                    p,
                    "midir-read-input",
                    move |stamp, message, _| {
                        info!("{}: {:?} (len = {})", stamp, message, message.len());
                        let result = if message.len() == 3 && message[0] == 144 {
                            MidiMessage::NoteOn(message[1], message[2])
                        } else if message.len() == 3 && message[0] == 128 {
                            MidiMessage::NoteOff(message[1], message[2])
                        } else {
                            MidiMessage::Unimplemented
                        };
                        port_tx.send(result).unwrap();
                    },
                    (),
                )
                .unwrap();
            midi_connections.push(conn_in);
        }

        thread::spawn(move || {
            let mut module = Module::new(
                NativeInterface::new(0, 3).unwrap(),
                rand::thread_rng(),
                "Midi_to_cv".into(),
                0,
            );
            let start = Instant::now();
            let mut time = 0;

            'outer: loop {
                while time < start.elapsed().as_millis() {
                    match rx.try_recv() {
                        Ok(message) => info!("{:?}", message),
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }
                    module.poll(start.elapsed().as_millis() as i64).unwrap();
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        MidiToCv {
            name,
            open: true,
            _midi_connections: midi_connections,
            note_checked: false,
            gate_checked: false,
            mdwh_checked: false,
        }
    }
}

impl DisplayModule for MidiToCv {
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
                ui.heading("Midi to CV");
                ui.add_space(20.0);
                ui.checkbox(&mut self.note_checked, "Note");
                ui.checkbox(&mut self.gate_checked, "Gate");
                ui.checkbox(&mut self.mdwh_checked, "Mod wheel");
            });
    }
}
