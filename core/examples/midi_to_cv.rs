use apiary_core::{midi_note_to_voct, AudioFrame, AudioPacket, BLOCK_SIZE, CHANNELS};
use midir::{MidiInput, MidiInputConnection};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use crate::display_module::{DisplayModule, Processor};

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

pub struct MidiToCv {
    voices: [Voice; CHANNELS],
    time: i64,
    rx: Receiver<MidiMessage>,
    _midi_connections: Vec<MidiInputConnection<()>>,
}

const NUM_PARAMS: usize = 0;

const NUM_INPUTS: usize = 0;

const NOTE_OUTPUT: usize = 0;
const GATE_OUTPUT: usize = 1;
const MDWH_OUTPUT: usize = 2;
const NUM_OUTPUTS: usize = 3;

impl MidiToCv {
    pub fn init() -> DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> {
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

        DisplayModule::new()
            .name("Midi to CV")
            .output(NOTE_OUTPUT, "Note")
            .output(GATE_OUTPUT, "Gate")
            .output(MDWH_OUTPUT, "Mod Wheel")
            .start(MidiToCv {
                voices: Default::default(),
                time: 0,
                rx: midi_rx,
                _midi_connections: midi_connections,
            })
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for MidiToCv {
    fn process(
        &mut self,
        _input: &[AudioPacket; NUM_INPUTS],
        output: &mut [AudioPacket; NUM_OUTPUTS],
        _params: &[f32; NUM_PARAMS],
    ) {
        match self.rx.try_recv() {
            Ok(message) => {
                trace!("{:?}", message);
                match message {
                    MidiMessage::NoteOff(_, note, _) => {
                        for v in self.voices.iter_mut().filter(|v| v.note == note && v.on) {
                            v.on = false;
                            v.timestamp = self.time;
                        }
                    }
                    MidiMessage::NoteOn(_, note, _) => {
                        // First, see if we can take the oldest voice that has been
                        // released. Otherwise, steal a voice. In this case, take the
                        // oldest note played. We also have a choice of whether to just
                        // change the pitch (done here), or to shut the note off and
                        // retrigger.
                        if let Some(v) = self.voices.iter_mut().min_by_key(|v| (v.on, v.timestamp))
                        {
                            v.note = note;
                            v.on = true;
                            v.timestamp = self.time;
                        }
                    }
                    _ => {}
                }
                for v in self.voices {
                    trace!("{:?}", v);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("Midi Disconnected"),
        }
        let mut note_frame: AudioFrame = Default::default();
        let mut gate_frame: AudioFrame = Default::default();
        for i in 0..CHANNELS {
            note_frame.data[i] = midi_note_to_voct(self.voices[i].note);
            if self.voices[i].on {
                gate_frame.data[i] = 16000;
            }
        }
        output[NOTE_OUTPUT] = AudioPacket {
            data: [note_frame; BLOCK_SIZE],
        };
        output[GATE_OUTPUT] = AudioPacket {
            data: [gate_frame; BLOCK_SIZE],
        };
        self.time += 1;
    }
}
