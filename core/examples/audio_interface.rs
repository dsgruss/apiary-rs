use apiary_core::{AudioFrame, Module};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Sample, SampleFormat, Stream, StreamConfig,
};
use eframe::egui;
use std::{
    error::Error,
    sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant},
};

use crate::common::{DisplayModule, Jack, SelectedInterface};

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
                    if dropped_frames != 0 {
                        info!("Audio dropped frames: {:?}", dropped_frames);
                        dropped_frames = 0;
                    }
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
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No default host device found",
        ))?;
        let mut configs = device.supported_output_configs()?;
        let supported_config = configs
            .next()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No supported configs found",
            ))?
            .with_max_sample_rate();
        info!("{:?}: {:?}", device.name()?, supported_config);

        let sample_format = supported_config.sample_format();
        let config = supported_config.into();

        let (audio_tx, audio_rx): (SyncSender<AudioFrame>, Receiver<AudioFrame>) =
            sync_channel(2000);

        let audio_stream = match sample_format {
            SampleFormat::F32 => run::<f32>(&device, &config, audio_rx),
            SampleFormat::I16 => run::<i16>(&device, &config, audio_rx),
            SampleFormat::U16 => run::<u16>(&device, &config, audio_rx),
        };

        audio_stream.play()?;

        let (ui_tx, ui_rx): (Sender<bool>, Receiver<bool>) = channel();

        thread::spawn(move || process(ui_rx, audio_tx));

        Ok(AudioInterface {
            width: 5.0,
            open: true,
            input_checked: false,
            tx: ui_tx,
            _audio_stream: audio_stream,
        })
    }
}

fn process(ui_rx: Receiver<bool>, audio_tx: SyncSender<AudioFrame>) {
    let start = Instant::now();
    let mut time: i64 = 0;
    let mut dropped_frames = 0;

    let mut module: Module<_, _, 1, 0> = Module::new(
        SelectedInterface::new().unwrap(),
        rand::thread_rng(),
        "audio_interface".into(),
        time,
    );

    'outer: loop {
        while time < start.elapsed().as_millis() as i64 {
            if time % 10000 == 0 {
                if dropped_frames != 0 {
                    info!("Module dropped frames: {:?}", dropped_frames);
                    dropped_frames = 0;
                }
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
            if let Err(e) = self.tx.send(self.input_checked) {
                info!("Ui channel closed: {:?}", e);
                self.open = false;
            }
        }
    }
}
