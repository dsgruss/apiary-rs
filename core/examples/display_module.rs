use apiary_core::{AudioPacket, Module};
use cpal::Stream;
use eframe::egui;
use std::{
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::common::{Jack, Knob, SelectedInterface};

pub struct DisplayModule<const I: usize, const O: usize, const P: usize> {
    name: String,
    width: f32,
    open: bool,
    tx: Option<Sender<PatchUpdate>>,
    s: Option<Stream>,
    params: Vec<Option<Param>>,
    inputs: Vec<String>,
    input_checks: [bool; I],
    outputs: Vec<String>,
    output_checks: [bool; O],
}

impl<const I: usize, const O: usize, const P: usize> DisplayModule<I, O, P> {
    pub fn new() -> Self {
        DisplayModule {
            name: "".into(),
            width: 5.0,
            open: true,
            tx: None,
            s: None,
            params: (0..P).map(|_| None).collect(),
            inputs: (0..I).map(|i| format!("Input {}", i)).collect(),
            input_checks: [false; I],
            outputs: (0..O).map(|i| format!("Output {}", i)).collect(),
            output_checks: [false; O],
        }
    }

    pub fn name(mut self, s: &str) -> Self {
        self.name = s.into();
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn param(
        mut self,
        id: usize,
        min: f32,
        max: f32,
        default: f32,
        name: &str,
        unit: &str,
        log: bool,
    ) -> Self {
        if id < P {
            self.params[id] = Some(Param {
                min,
                max,
                val: default.clamp(min, max),
                name: name.into(),
                unit: unit.into(),
                log,
            });
        }
        self
    }

    pub fn input(mut self, id: usize, name: &str) -> Self {
        if id < I {
            self.inputs[id] = name.into();
        }
        self
    }

    pub fn output(mut self, id: usize, name: &str) -> Self {
        if id < O {
            self.outputs[id] = name.into();
        }
        self
    }

    pub fn stream_store(mut self, s: Stream) -> Self {
        // The handle to the audio interface is not able to be passed to the processing thread, so
        // we need to have a place to keep it so that it does not drop when it falls out of scope.
        self.s = Some(s);
        self
    }

    pub fn start<T>(mut self, p: T) -> Self
    where
        T: Processor<I, O, P> + Send + 'static,
    {
        let (ui_tx, ui_rx): (Sender<PatchUpdate>, Receiver<PatchUpdate>) = channel();
        self.tx = Some(ui_tx);
        let name = self.name.clone();
        let mut params = [0.0; P];
        for i in 0..P {
            if let Some(v) = &self.params[i] {
                params[i] = v.val;
            }
        }
        thread::spawn(move || process(ui_rx, &name, params, p));
        self
    }
}

struct Param {
    min: f32,
    max: f32,
    val: f32,
    name: String,
    unit: String,
    log: bool,
}

fn process<const I: usize, const O: usize, const P: usize, T: Processor<I, O, P>>(
    rx: Receiver<PatchUpdate>,
    name: &str,
    mut params: [f32; P],
    mut p: T,
) {
    let start = Instant::now();
    let mut time: i64 = 0;

    let mut module: Module<_, _, I, O> = Module::new(
        SelectedInterface::new().unwrap(),
        rand::thread_rng(),
        name.into(),
        time,
    );

    'outer: loop {
        while time < start.elapsed().as_millis() as i64 {
            match rx.try_recv() {
                Ok(PatchUpdate::Input(id, on)) => {
                    if let Err(e) = module.set_input_patch_enabled(id, on) {
                        info!("Error {:?}", e);
                    }
                }
                Ok(PatchUpdate::Output(id, on)) => {
                    if let Err(e) = module.set_output_patch_enabled(id, on) {
                        info!("Error {:?}", e);
                    }
                }
                Ok(PatchUpdate::Param(id, val)) => {
                    params[id] = val;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => break 'outer,
            }
            module
                .poll(time, |input, output| {
                    p.process(input, output, &params);
                })
                .unwrap();
            time += 1;
        }
        thread::sleep(Duration::from_millis(0));
    }
}

impl<const I: usize, const O: usize, const P: usize> DisplayHandler for DisplayModule<I, O, P> {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        if let Some(tx) = &self.tx {
            ui.heading(self.name.clone());
            ui.add_space(20.0);
            // Add ui and message transmission
            for i in 0..I {
                if ui
                    .add(Jack::new(&mut self.input_checks[i], self.inputs[i].clone()))
                    .changed()
                {
                    self.open &= tx.send(PatchUpdate::Input(i, self.input_checks[i])).is_ok();
                }
            }
            ui.add_space(20.0);
            for (i, p) in self.params.iter_mut().enumerate() {
                if let Some(p) = p {
                    ui.add(Knob::new(
                        &mut p.val,
                        p.name.clone(),
                        p.unit.clone(),
                        p.min,
                        p.max,
                        p.log,
                    ));
                    self.open &= tx.send(PatchUpdate::Param(i, p.val)).is_ok();
                }
            }
            ui.add_space(20.0);
            for i in 0..O {
                if ui
                    .add(Jack::new(
                        &mut self.output_checks[i],
                        self.outputs[i].clone(),
                    ))
                    .changed()
                {
                    self.open = tx
                        .send(PatchUpdate::Output(i, self.output_checks[i]))
                        .is_ok();
                }
            }
        }
    }
}

pub trait DisplayHandler {
    fn width(&self) -> f32;
    fn is_open(&self) -> bool;
    fn update(&mut self, ui: &mut egui::Ui);
}

pub trait Processor<const I: usize, const O: usize, const P: usize> {
    fn process(
        &mut self,
        input: &[AudioPacket; I],
        output: &mut [AudioPacket; O],
        params: &[f32; P],
    );
}

#[derive(Debug)]
enum PatchUpdate {
    Input(usize, bool),
    Output(usize, bool),
    Param(usize, f32),
}
