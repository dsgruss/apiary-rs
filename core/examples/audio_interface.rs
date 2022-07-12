use apiary_core::{socket_native::NativeInterface, AudioFrame, Module};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Sample, SampleFormat, Stream, StreamConfig,
};
use eframe::egui;
use std::{
    sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack};

pub struct AudioInterface {
    width: f32,
    open: bool,
    tx: Sender<bool>,
    input_checked: bool,
    _audio_stream: Stream,
}

fn run<T>(device: &Device, config: &StreamConfig, audio_rx: Receiver<AudioFrame>) -> Stream
where
    T: Sample,
{
    let mut start = Instant::now();
    let mut dropped_frames = 0;
    device
        .build_output_stream(
            &config,
            move |data: &mut [T], _| {
                if start.elapsed().as_secs() >= 10 {
                    info!("Audio dropped frames: {:?}", dropped_frames);
                    start = Instant::now();
                }
                for i in 0..(data.len() / 2) {
                    let sample = match audio_rx.try_recv() {
                        Ok(v) => {
                            let mut avg: i32 = 0;
                            for val in v.data {
                                avg += val as i32;
                            }
                            let sample = (avg >> 3) as i16;
                            Sample::from(&sample)
                        }
                        Err(TryRecvError::Empty) => {
                            dropped_frames += 1;
                            Sample::from(&0.0)
                        }
                        Err(TryRecvError::Disconnected) => {
                            panic!("Audio channel disconnected")
                        }
                    };
                    data[2 * i] = sample;
                    data[2 * i + 1] = sample;
                }
            },
            |err| info!("Audio stream error: {:?}", err),
        )
        .unwrap()
}

impl AudioInterface {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().unwrap();
        let mut configs = device.supported_output_configs().unwrap();
        let supported_config = configs.next().unwrap().with_max_sample_rate();
        info!("{:?}: {:?}", device.name().unwrap(), supported_config);

        let sample_format = supported_config.sample_format();
        let config = supported_config.into();

        let (audio_tx, audio_rx): (SyncSender<AudioFrame>, Receiver<AudioFrame>) =
            sync_channel(960);

        let audio_stream = match sample_format {
            SampleFormat::F32 => run::<f32>(&device, &config, audio_rx),
            SampleFormat::I16 => run::<i16>(&device, &config, audio_rx),
            SampleFormat::U16 => run::<u16>(&device, &config, audio_rx),
        };

        audio_stream.play().unwrap();

        let (ui_tx, ui_rx): (Sender<bool>, Receiver<bool>) = channel();

        thread::spawn(move || {
            let mut module: Module<_, _, 1, 0> = Module::new(
                NativeInterface::new().unwrap(),
                rand::thread_rng(),
                "audio_interface".into(),
                0,
            );
            let start = Instant::now();
            let mut time: i64 = 0;
            let mut dropped_frames = 0;

            'outer: loop {
                while time < start.elapsed().as_millis() as i64 {
                    if time % 10000 == 0 {
                        info!("Module dropped frames: {:?}", dropped_frames);
                    }
                    match ui_rx.try_recv() {
                        Ok(checked) => {
                            module.set_input_patch_enabled(0, checked).unwrap();
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => break 'outer,
                    }
                    module
                        .poll(time, |input, _| {
                            for frame in input[0].data {
                                match audio_tx.try_send(frame) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(_)) => {
                                        dropped_frames += 1;
                                    }
                                    Err(TrySendError::Disconnected(_)) => {
                                        panic!("Audio channel disconnected")
                                    }
                                }
                            }
                        })
                        .unwrap();
                    time += 1;
                }
                thread::sleep(Duration::from_millis(0));
            }
        });

        AudioInterface {
            width: 5.0,
            open: true,
            input_checked: false,
            tx: ui_tx,
            _audio_stream: audio_stream,
        }
    }
}

impl DisplayModule for AudioInterface {
    fn width(&self) -> f32 {
        self.width
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        ui.heading("Audio Interface");
        ui.add_space(20.0);
        if ui
            .add(Jack::new(&mut self.input_checked, "Input"))
            .changed()
        {
            self.tx.send(self.input_checked).unwrap();
        }
    }
}
