use apiary_core::{socket_native::NativeInterface, Module, CHANNELS};
use eframe::egui::{
    self,
    plot::{Line, Plot, Value, Values},
};
use std::{
    sync::{
        mpsc::{channel, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack};

pub struct Oscilloscope {
    width: f32,
    open: bool,
    tx: Sender<bool>,
    input_checked: bool,
    data: Arc<Mutex<[Vec<Value>; CHANNELS]>>,
}

impl Oscilloscope {
    pub fn new() -> Self {
        let (ui_tx, ui_rx): (Sender<bool>, Receiver<bool>) = channel();
        let data: Arc<Mutex<[Vec<Value>; CHANNELS]>> = Default::default();
        let thread_data = data.clone();

        thread::spawn(move || {
            let mut module: Module<_, _, 1, 0> = Module::new(
                NativeInterface::new().unwrap(),
                rand::thread_rng(),
                "Oscilloscope".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;

            'outer: loop {
                while time < start.elapsed().as_millis() as i64 {
                    match ui_rx.try_recv() {
                        Ok(checked) => {
                            if let Err(e) = module.set_input_patch_enabled(0, checked) {
                                info!("Error in connecting jack: {:?}", e);
                            }
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }

                    let mut pkt = Default::default();
                    module
                        .poll(time, |input, _| {
                            pkt = input[0];
                        })
                        .unwrap();
                    if time % 10 == 0 {
                        let mut data = thread_data.lock().unwrap();
                        for i in 0..CHANNELS {
                            data[i]
                                .push(Value::new(time as f64 / 1000.0, pkt.data[0].data[i] as f64));
                            if data[i].len() > 400 {
                                data[i].remove(0);
                            }
                        }
                    }
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        Oscilloscope {
            width: 25.0,
            open: true,
            tx: ui_tx,
            input_checked: false,
            data: data,
        }
    }
}

impl DisplayModule for Oscilloscope {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("Oscilloscope");
        ui.add_space(20.0);

        let inner_data = self.data.lock().unwrap();
        Plot::new("my_plot")
            .width(350.0)
            .height(350.0)
            .show(ui, |plot_ui| {
                for i in 0..CHANNELS {
                    let line = Line::new(Values::from_values(inner_data[i].clone()));
                    plot_ui.line(line);
                }
            });

        if ui
            .add(Jack::new(&mut self.input_checked, "Input"))
            .changed()
        {
            self.tx.send(self.input_checked).unwrap();
        }
    }
}
