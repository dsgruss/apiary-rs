use core::iter::zip;

use apiary_core::{
    dsp::LinearTrap, softclip, voct_to_freq_scale, AudioPacket, InputJackHandle, Module, Network,
    OutputJackHandle, PollUpdate, ProcessBlock, CHANNELS,
};
use itertools::izip;
use libm::{log10f, powf};
use palette::Srgb;
use rand_core::RngCore;
use stm32f4xx_hal::gpio;

use crate::ui::Switch;

pub const NUM_INPUTS: usize = 3;
pub const NUM_OUTPUTS: usize = 1;
pub const COLOR: u16 = 220;
pub const NAME: &str = "filter";

pub struct FilterPins {
    pub input: gpio::Pin<'C', 8>,
    pub key_track: gpio::Pin<'C', 9>,
    pub contour: gpio::Pin<'D', 12>,
    pub output: gpio::Pin<'D', 13>,
}

pub struct Filter {
    input: Switch<'C', 8>,
    key_track: Switch<'C', 9>,
    contour: Switch<'D', 12>,
    output: Switch<'D', 13>,
    filters: [LinearTrap; CHANNELS],
    jack_input: InputJackHandle,
    jack_key_track: InputJackHandle,
    jack_contour: InputJackHandle,
    jack_output: OutputJackHandle,
    params: [f32; 3],
}

impl Filter {
    pub fn new<T, R>(pins: FilterPins, module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>) -> Self
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        Filter {
            input: Switch::new(pins.input),
            key_track: Switch::new(pins.key_track),
            contour: Switch::new(pins.contour),
            output: Switch::new(pins.output),
            filters: Default::default(),
            jack_input: module.add_input_jack().unwrap(),
            jack_key_track: module.add_input_jack().unwrap(),
            jack_contour: module.add_input_jack().unwrap(),
            jack_output: module.add_output_jack().unwrap(),
            params: [0.0; 3],
        }
    }

    pub fn poll_ui<T, R>(&mut self, module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>)
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        self.input.debounce();
        self.key_track.debounce();
        self.contour.debounce();
        self.output.debounce();
        if self.input.just_pressed() {
            info!("input switch pressed");
        }
        if self.input.released() {
            info!("input switch released");
        }
        if self.key_track.just_pressed() {
            info!("key_track switch pressed");
        }
        if self.key_track.released() {
            info!("key_track switch released");
        }
        if self.contour.just_pressed() {
            info!("contour switch pressed");
        }
        if self.contour.released() {
            info!("contour switch released");
        }
        if self.output.just_pressed() {
            info!("output switch pressed");
        }
        if self.output.released() {
            info!("output switch released");
        }

        if self.input.changed()
            || self.key_track.changed()
            || self.contour.changed()
            || self.output.changed()
        {
            module
                .set_input_patch_enabled(self.jack_input, self.input.just_pressed())
                .unwrap();
            module
                .set_input_patch_enabled(self.jack_key_track, self.key_track.just_pressed())
                .unwrap();
            module
                .set_input_patch_enabled(self.jack_contour, self.contour.just_pressed())
                .unwrap();
            module
                .set_output_patch_enabled(self.jack_output, self.output.just_pressed())
                .unwrap();
        }
    }

    pub fn process(&mut self, block: &mut ProcessBlock<NUM_INPUTS, NUM_OUTPUTS>) {
        // Processing time is too slow to do this every audio frame...
        for i in 0..CHANNELS {
            self.filters[i].set_params(
                self.params[0]
                    * voct_to_freq_scale(
                        block.get_input(self.jack_key_track).data[0].data[i] as f32
                            + block.get_input(self.jack_contour).data[0].data[i] as f32
                                / i16::MAX as f32
                                * self.params[2]
                                * 512.0
                                * 12.0
                                * 4.0,
                    ),
                self.params[1],
            );
        }
        let mut output: AudioPacket = Default::default();
        for (fin, fout) in zip(
            block.get_input(self.jack_input).data,
            output.data.iter_mut(),
        ) {
            for (iin, iout, filter) in
                izip!(fin.data, fout.data.iter_mut(), self.filters.iter_mut())
            {
                *iout = (softclip(filter.process(iin as f32 / i16::MAX as f32)) * i16::MAX as f32)
                    as i16;
            }
        }
        block.set_output(self.jack_output, output);
    }

    pub fn set_params(&mut self, adc: &mut [u16; 8]) {
        self.params[0] += 0.01
            * (20.0 * powf(10.0, (adc[0] as f32 / 4096.0) * log10f(8000.0 / 20.0))
                - self.params[0]);
        self.params[1] = powf(adc[1] as f32 / 4096.0, 2.0) * 10.0;
        self.params[2] = adc[2] as f32 / 4096.0;
    }

    pub fn get_light_data(&self, update: PollUpdate<NUM_INPUTS, NUM_OUTPUTS>) -> [Srgb<u8>; 4] {
        [
            update.get_input_color(self.jack_key_track),
            update.get_input_color(self.jack_contour),
            update.get_input_color(self.jack_input),
            update.get_output_color(self.jack_output),
        ]
    }
}
