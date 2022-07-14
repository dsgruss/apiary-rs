use apiary_core::{midi_note_to_voct, AudioFrame, AudioPacket, Module, BLOCK_SIZE, CHANNELS};
use eframe::egui;
use midir::{MidiInput, MidiInputConnection};
use std::{
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack, SelectedInterface, UiUpdate};

pub struct MidiToCv {
    width: f32,
    open: bool,
    _midi_connections: Vec<MidiInputConnection<()>>,
    note_checked: bool,
    gate_checked: bool,
    mdwh_checked: bool,
    tx: Sender<UiUpdate>,
}

#[derive(Debug)]
enum MidiMessage {
    NoteOn(u8, u8, u8),
    NoteOff(u8, u8, u8),
    Unimplemented,
}

#[derive(Copy, Clone, Default, Debug)]
struct Voice {
    note: u8,
    on: bool,
    timestamp: i64,
}

impl MidiToCv {
    pub fn new() -> Self {
        let (midi_tx, midi_rx) = channel();

        let midi_check = MidiInput::new("midir port enumerator").unwrap();
        info!("Opening all MIDI inputs by default");

        let in_ports = midi_check.ports();
        let mut midi_connections = vec![];
        for (i, port) in in_ports.iter().enumerate() {
            info!("Opening {:?}", midi_check.port_name(port).unwrap());
            let midi_in = MidiInput::new(&format!("midir input {}", i)).unwrap();
            let port_tx = midi_tx.clone();
            let port_name = "midir-read-input";

            let conn_in = midi_in
                .connect(
                    port,
                    port_name,
                    move |_, msg, _| midi_dispatch(msg, &port_tx),
                    (),
                )
                .unwrap();
            midi_connections.push(conn_in);
        }

        let (ui_tx, ui_rx): (Sender<UiUpdate>, Receiver<UiUpdate>) = channel();

        thread::spawn(move || process(midi_rx, ui_rx));

        MidiToCv {
            width: 10.0,
            open: true,
            _midi_connections: midi_connections,
            note_checked: false,
            gate_checked: false,
            mdwh_checked: false,
            tx: ui_tx,
        }
    }
}

fn midi_dispatch(message: &[u8], tx: &Sender<MidiMessage>) {
    let result = if message.len() != 3 {
        MidiMessage::Unimplemented
    } else {
        match (message[0] >> 4, message[0] & 0b1111) {
            (0b1001, ch) => MidiMessage::NoteOn(ch, message[1], message[2]),
            (0b1000, ch) => MidiMessage::NoteOff(ch, message[1], message[2]),
            _ => MidiMessage::Unimplemented,
        }
    };
    tx.send(result).unwrap();
}

fn process(midi_rx: Receiver<MidiMessage>, ui_rx: Receiver<UiUpdate>) {
    let start = Instant::now();
    let mut time: i64 = 0;
    let mut voices: [Voice; CHANNELS] = [Default::default(); CHANNELS];

    let mut module: Module<_, _, 0, 3> = Module::new(
        SelectedInterface::new().unwrap(),
        rand::thread_rng(),
        "Midi_to_cv".into(),
        time,
    );

    'outer: loop {
        while time < start.elapsed().as_millis() as i64 {
            match midi_rx.try_recv() {
                Ok(message) => {
                    trace!("{:?}", message);
                    match message {
                        MidiMessage::NoteOff(_, note, _) => {
                            for v in voices.iter_mut().filter(|v| v.note == note && v.on) {
                                v.on = false;
                                v.timestamp = time;
                            }
                        }
                        MidiMessage::NoteOn(_, note, _) => {
                            // First, see if we can take the oldest voice that has been
                            // released. Otherwise, steal a voice. In this case, take the
                            // oldest note played. We also have a choice of whether to just
                            // change the pitch (done here), or to shut the note off and
                            // retrigger.
                            if let Some(v) = voices.iter_mut().min_by_key(|v| (v.on, v.timestamp)) {
                                v.note = note;
                                v.on = true;
                                v.timestamp = time;
                            }
                        }
                        _ => {}
                    }
                    for v in voices {
                        trace!("{:?}", v);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => break 'outer,
            }
            match ui_rx.try_recv() {
                Ok(msg) => {
                    if msg.input {
                        if let Err(e) = module.set_input_patch_enabled(msg.id, msg.on) {
                            info!("Error {:?} {:?}", e, msg);
                        }
                    } else {
                        if let Err(e) = module.set_output_patch_enabled(msg.id, msg.on) {
                            info!("Error {:?} {:?}", e, msg);
                        }
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => break 'outer,
            }
            let mut note_frame: AudioFrame = Default::default();
            let mut gate_frame: AudioFrame = Default::default();
            for i in 0..CHANNELS {
                note_frame.data[i] = midi_note_to_voct(voices[i].note);
                if voices[i].on {
                    gate_frame.data[i] = 16000;
                }
            }
            let note_pkt = AudioPacket {
                data: [note_frame; BLOCK_SIZE],
            };
            let gate_pkt = AudioPacket {
                data: [gate_frame; BLOCK_SIZE],
            };
            module
                .poll(time, |_, output| {
                    output[0] = note_pkt;
                    output[1] = gate_pkt;
                })
                .unwrap();
            time += 1;
        }
        thread::sleep(Duration::from_millis(0));
    }
}

impl DisplayModule for MidiToCv {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("Midi to CV");
        ui.add_space(20.0);
        if ui.add(Jack::new(&mut self.note_checked, "Note")).changed() {
            self.tx
                .send(UiUpdate::new(false, 0, self.note_checked))
                .unwrap();
        }
        if ui.add(Jack::new(&mut self.gate_checked, "Gate")).changed() {
            self.tx
                .send(UiUpdate::new(false, 1, self.gate_checked))
                .unwrap();
        }
        if ui
            .add(Jack::new(&mut self.mdwh_checked, "Mod wheel"))
            .changed()
        {
            self.tx
                .send(UiUpdate::new(false, 2, self.mdwh_checked))
                .unwrap();
        }
    }
}
