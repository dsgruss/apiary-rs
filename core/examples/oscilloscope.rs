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

use crate::common::DisplayModule;

pub struct Oscilloscope {
    name: String,
    open: bool,
    tx: Sender<String>,
    address: String,
    data: Arc<Mutex<[Vec<Value>; CHANNELS]>>,
}

impl Oscilloscope {
    pub fn new(name: String) -> Self {
        let (ui_tx, ui_rx): (Sender<String>, Receiver<String>) = channel();
        let data: Arc<Mutex<[Vec<Value>; CHANNELS]>> = Default::default();
        let thread_data = data.clone();

        thread::spawn(move || {
            let mut module: Module<_, _, 1, 0> = Module::new(
                NativeInterface::new(1, 0).unwrap(),
                rand::thread_rng(),
                "Oscilloscope".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;

            'outer: loop {
                while time < start.elapsed().as_millis() as i64 {
                    match ui_rx.try_recv() {
                        Ok(message) => {
                            info!("Connecting jack to {:?}", message);
                            if let Err(e) = module.jack_connect(0, &message, time) {
                                info!("Error in connecting jack: {:?}", e);
                            }
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }

                    let mut pkt = Default::default();
                    module.poll(time, |input, _| {
                        pkt = input[0];
                    }).unwrap();
                    if time % 10 == 0 {
                        let mut data = thread_data.lock().unwrap();
                        for i in 0..CHANNELS {
                            data[i].push(Value::new(
                                time as f64 / 1000.0,
                                pkt.data[0].data[i] as f64,
                            ));
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
            name,
            open: true,
            tx: ui_tx,
            address: "".to_owned(),
            data: data,
        }
    }
}

impl DisplayModule for Oscilloscope {
    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ctx: &egui::Context) {
        egui::Window::new(&self.name)
            .open(&mut self.open)
            .collapsible(false)
            .resizable(false)
            .min_height(450.0)
            .min_width(450.0)
            .show(ctx, |ui| {
                ui.heading("Oscilloscope");
                ui.add_space(20.0);

                let inner_data = self.data.lock().unwrap();
                Plot::new("my_plot").view_aspect(1.0).show(ui, |plot_ui| {
                    for i in 0..CHANNELS {
                        let line = Line::new(Values::from_values(inner_data[i].clone()));
                        plot_ui.line(line);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Multicast Address: ");
                    ui.text_edit_singleline(&mut self.address);
                    if ui.button("Connect").clicked() {
                        self.tx.send(self.address.clone()).unwrap();
                    }
                });

                ui.allocate_space(ui.available_size());
            });
        ctx.request_repaint();
    }
}
