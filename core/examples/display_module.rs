use apiary_core::{AudioPacket, Module};
use cpal::Stream;
use eframe::egui;
use palette::Srgb;
use rand::Rng;
use std::{
    sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant}, iter::zip,
};

use crate::common::{Jack, Knob, SelectedInterface};

pub struct DisplayModule<const I: usize, const O: usize, const P: usize> {
    name: String,
    color: u16,
    width: f32,
    open: bool,
    tx: Option<Sender<PatchUpdate>>,
    rx: Option<Receiver<([Srgb<u8>; I], [Srgb<u8>; O])>>,
    s: Option<Stream>,
    renderer: Option<Box<dyn Renderer<I, O, P>>>,
    params: Vec<Option<Param>>,
    inputs: Vec<String>,
    input_checks: [bool; I],
    input_colors: [Srgb<u8>; I],
    outputs: Vec<String>,
    output_checks: [bool; O],
    output_colors: [Srgb<u8>; O],
}

impl<const I: usize, const O: usize, const P: usize> DisplayModule<I, O, P> {
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        DisplayModule {
            name: "".into(),
            width: 5.0,
            color: rng.gen_range(0..360),
            open: true,
            tx: None,
            rx: None,
            s: None,
            renderer: None,
            params: (0..P).map(|_| None).collect(),
            inputs: (0..I).map(|i| format!("Input {}", i)).collect(),
            input_checks: [false; I],
            input_colors: [Srgb::new(64, 254, 0); I],
            outputs: (0..O).map(|i| format!("Output {}", i)).collect(),
            output_checks: [false; O],
            output_colors: [Srgb::new(64, 254, 0); O],
        }
    }

    pub fn name(mut self, s: &str) -> Self {
        self.name = s.into();
        self
    }

    pub fn color(mut self, color: u16) -> Self {
        self.color = color;
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

    pub fn renderer<R>(mut self, renderer: R) -> Self
    where
        R: Renderer<I, O, P> + 'static,
    {
        self.renderer = Some(Box::new(renderer));
        self
    }

    pub fn start<T>(mut self, p: T) -> Self
    where
        T: Processor<I, O, P> + Send + 'static,
    {
        let (ui_tx, ui_rx): (Sender<PatchUpdate>, Receiver<PatchUpdate>) = channel();
        let (color_tx, color_rx) = sync_channel(1);
        self.tx = Some(ui_tx);
        self.rx = Some(color_rx);
        let name = self.name.clone();
        let mut params = [0.0; P];
        for i in 0..P {
            if let Some(v) = &self.params[i] {
                params[i] = v.val;
            }
        }
        thread::spawn(move || process(ui_rx, color_tx, &name, self.color, params, p));
        self
    }

    pub fn input_jack(&mut self, id: usize, ui: &mut egui::Ui) {
        if let Some(tx) = &self.tx {
            if ui
                .add(Jack::new(
                    &mut self.input_checks[id],
                    self.inputs[id].clone(),
                    self.input_colors[id],
                ))
                .changed()
            {
                self.open &= tx
                    .send(PatchUpdate::Input(id, self.input_checks[id]))
                    .is_ok();
            }
        }
    }

    pub fn param_knob(&mut self, id: usize, ui: &mut egui::Ui) {
        if let Some(tx) = &self.tx {
            if let Some(p) = &mut self.params[id] {
                ui.add(Knob::new(
                    &mut p.val,
                    p.name.clone(),
                    p.unit.clone(),
                    p.min,
                    p.max,
                    p.log,
                ));
                self.open &= tx.send(PatchUpdate::Param(id, p.val)).is_ok();
            }
        }
    }

    pub fn output_jack(&mut self, id: usize, ui: &mut egui::Ui) {
        if let Some(tx) = &self.tx {
            if ui
                .add(Jack::new(
                    &mut self.output_checks[id],
                    self.outputs[id].clone(),
                    self.output_colors[id],
                ))
                .changed()
            {
                self.open = tx
                    .send(PatchUpdate::Output(id, self.output_checks[id]))
                    .is_ok();
            }
        }
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
    tx: SyncSender<([Srgb<u8>; I], [Srgb<u8>; O])>,
    name: &str,
    color: u16,
    mut params: [f32; P],
    mut p: T,
) {
    let start = Instant::now();
    let mut time: i64 = 0;

    let mut module: Module<_, _, I, O> = Module::new(
        SelectedInterface::new().unwrap(),
        rand::thread_rng(),
        name.into(),
        color,
        time,
    );
    let input_handles = [0; I].map(|_| module.add_input_jack().unwrap());
    let output_handles = [0; O].map(|_| module.add_output_jack().unwrap());

    'outer: loop {
        while time < start.elapsed().as_millis() as i64 {
            match rx.try_recv() {
                Ok(PatchUpdate::Input(id, on)) => {
                    if let Err(e) = module.set_input_patch_enabled(input_handles[id], on) {
                        info!("Error {:?}", e);
                    }
                }
                Ok(PatchUpdate::Output(id, on)) => {
                    if let Err(e) = module.set_output_patch_enabled(output_handles[id], on) {
                        info!("Error {:?}", e);
                    }
                }
                Ok(PatchUpdate::Param(id, val)) => {
                    params[id] = val;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => break 'outer,
            }
            let res = module
                .poll(time, |block| {
                    let input = input_handles.map(|h| block.get_input(h));
                    let mut output = [Default::default(); O];
                    p.process(input, &mut output, &params);
                    for (h, o) in zip(output_handles, output) {
                        block.set_output(h, o);
                    }
                })
                .unwrap();
            let colors = (input_handles.map(|h| res.get_input_color(h)),
        output_handles.map(|h| res.get_output_color(h)));
            if let Err(TrySendError::Disconnected(_)) = tx.try_send(colors) {
                break 'outer;
            }
            time += 1;
        }
        thread::sleep(Duration::from_millis(0));
    }
}

impl<const I: usize, const O: usize, const P: usize> DisplayHandler for DisplayModule<I, O, P> {
    fn width(&self) -> f32 {
        self.width
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.input_colors = res.0;
                    self.output_colors = res.1;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.open = false,
            }
        }
        ui.heading(self.name.clone());
        ui.add_space(20.0);
        // Add ui and message transmission
        for i in 0..I {
            self.input_jack(i, ui);
        }
        ui.add_space(20.0);
        for i in 0..P {
            self.param_knob(i, ui);
        }
        ui.add_space(20.0);
        for i in 0..O {
            self.output_jack(i, ui);
        }
    }
}

pub trait DisplayHandler {
    fn width(&self) -> f32;
    fn name(&self) -> &str;
    fn is_open(&self) -> bool;
    fn update(&mut self, ui: &mut egui::Ui);
}

pub trait AsDisplayModule<const I: usize, const O: usize, const P: usize> {
    fn as_display_module(&self) -> &DisplayModule<I, O, P>;
}

pub trait AsMutDisplayModule<const I: usize, const O: usize, const P: usize> {
    fn as_mut_display_module(&mut self) -> &mut DisplayModule<I, O, P>;
}

pub trait Processor<const I: usize, const O: usize, const P: usize> {
    fn process(
        &mut self,
        input: [&AudioPacket; I],
        output: &mut [AudioPacket; O],
        params: &[f32; P],
    );
}

pub trait Renderer<const I: usize, const O: usize, const P: usize> {
    fn render(&mut self, disp: &mut DisplayModule<I, O, P>, ui: &mut egui::Ui);
}

#[derive(Debug)]
enum PatchUpdate {
    Input(usize, bool),
    Output(usize, bool),
    Param(usize, f32),
}
