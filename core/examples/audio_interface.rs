use apiary_core::{softclip, AudioFrame, AudioPacket};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Sample, SampleFormat, Stream, StreamConfig,
};
use std::{
    error::Error,
    io,
    io::ErrorKind,
    sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError},
    time::Instant,
};

use crate::display_module::{DisplayModule, Processor};

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
                            let mut avg: f32 = 0.0;
                            for val in v.data {
                                avg += val as f32;
                            }
                            let sample = (softclip(avg / i16::MAX as f32) * i16::MAX as f32) as i16;
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

pub struct AudioInterface {
    time: i64,
    dropped_frames: i64,
    audio_tx: SyncSender<AudioFrame>,
}

const NUM_PARAMS: usize = 0;

const IN_INPUT: usize = 0;
const NUM_INPUTS: usize = 1;

const NUM_OUTPUTS: usize = 0;

impl AudioInterface {
    pub fn init() -> Result<DisplayModule<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS>, Box<dyn Error>> {
        let host = cpal::default_host();
        let mut found_device = None;
        for d in host.output_devices()? {
            info!("{:?}", d.name()?);
        }
        for d in host.output_devices()? {
            if d.name()?.contains("CABLE") {
                found_device = Some(d);
                break;
            }
        }
        let device = match found_device {
            Some(d) => d,
            None => host.default_output_device().ok_or(io::Error::new(
                ErrorKind::NotFound,
                "No default host device found",
            ))?,
        };
        let mut configs = device.supported_output_configs()?;
        let supported_config = configs
            .next()
            .ok_or(io::Error::new(
                ErrorKind::NotFound,
                "No supported configs found",
            ))?
            .with_max_sample_rate();
        info!(
            "Selecting device: {:?}: {:?}",
            device.name()?,
            supported_config
        );

        let sample_format = supported_config.sample_format();
        let config = supported_config.into();

        // Currently, the audio interface seems to be running every-so-slightly slower than the
        // expected 48,000 Hz (Dropping 48 frames or 1 ms every ten seconds on average), so we
        // increase the buffer size here to compensate.
        let (audio_tx, audio_rx): (SyncSender<AudioFrame>, Receiver<AudioFrame>) =
            sync_channel(960);

        let audio_stream = match sample_format {
            SampleFormat::F32 => run::<f32>(&device, &config, audio_rx),
            SampleFormat::I16 => run::<i16>(&device, &config, audio_rx),
            SampleFormat::U16 => run::<u16>(&device, &config, audio_rx),
        };

        audio_stream.play()?;

        Ok(DisplayModule::new()
            .name("Audio Interface")
            .input(IN_INPUT, "Input")
            .stream_store(audio_stream)
            .start(AudioInterface {
                time: 0,
                dropped_frames: 0,
                audio_tx,
            }))
    }
}

impl Processor<NUM_INPUTS, NUM_OUTPUTS, NUM_PARAMS> for AudioInterface {
    fn process(
        &mut self,
        input: &[AudioPacket; NUM_INPUTS],
        _output: &mut [AudioPacket; NUM_OUTPUTS],
        _params: &[f32; NUM_PARAMS],
    ) {
        if self.time % 10000 == 0 {
            if self.dropped_frames != 0 {
                info!("Module dropped frames: {:?}", self.dropped_frames);
                self.dropped_frames = 0;
            }
        }
        for frame in input[IN_INPUT].data {
            match self.audio_tx.try_send(frame) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.dropped_frames += 1;
                }
                Err(TrySendError::Disconnected(_)) => {
                    panic!("Audio channel disconnected")
                }
            }
        }
        self.time += 1;
    }
}
