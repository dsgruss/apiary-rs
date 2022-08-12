use apiary_core::{
    dsp::oscillators::WtOscillator, voct_to_frequency_table, InputJackHandle, Module, Network,
    OutputJackHandle, PollUpdate, ProcessBlock, BLOCK_SIZE, CHANNELS,
};
use palette::Srgb;
use rand_core::RngCore;
use stm32f4xx_hal::gpio;

use crate::ui::Switch;

pub const NUM_INPUTS: usize = 2;
pub const NUM_OUTPUTS: usize = 3;
pub const COLOR: u16 = 125;
pub const NAME: &str = "oscillator";

pub struct OscillatorPins {
    pub input: gpio::Pin<'C', 7>,
    pub level: gpio::Pin<'C', 8>,
    pub tri: gpio::Pin<'C', 9>,
    pub saw: gpio::Pin<'D', 12>,
    pub sqr: gpio::Pin<'D', 13>,
}

pub struct Oscillator {
    input: Switch<'C', 7>,
    level: Switch<'C', 8>,
    tri: Switch<'C', 9>,
    saw: Switch<'D', 12>,
    sqr: Switch<'D', 13>,
    osc: [WtOscillator; CHANNELS],
    jack_input: InputJackHandle,
    jack_level: InputJackHandle,
    jack_tri: OutputJackHandle,
    jack_saw: OutputJackHandle,
    jack_sqr: OutputJackHandle,
    // params: [f32; 3],
}

impl Oscillator {
    pub fn new<T, R>(
        pins: OscillatorPins,
        module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>,
    ) -> Self
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        Oscillator {
            input: Switch::new(pins.input),
            level: Switch::new(pins.level),
            tri: Switch::new(pins.tri),
            saw: Switch::new(pins.saw),
            sqr: Switch::new(pins.sqr),
            osc: Default::default(),
            jack_input: module.add_input_jack().unwrap(),
            jack_level: module.add_input_jack().unwrap(),
            jack_tri: module.add_output_jack().unwrap(),
            jack_saw: module.add_output_jack().unwrap(),
            jack_sqr: module.add_output_jack().unwrap(),
            // params: [0.0; 3],
        }
    }

    pub fn poll_ui<T, R>(&mut self, module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>)
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        self.input.debounce();
        self.level.debounce();
        self.tri.debounce();
        self.saw.debounce();
        self.sqr.debounce();

        if self.input.changed()
            || self.level.changed()
            || self.tri.changed()
            || self.saw.changed()
            || self.sqr.changed()
        {
            module
                .set_input_patch_enabled(self.jack_input, self.input.just_pressed())
                .unwrap();
            module
                .set_input_patch_enabled(self.jack_level, self.level.just_pressed())
                .unwrap();
            module
                .set_output_patch_enabled(self.jack_tri, self.tri.just_pressed())
                .unwrap();
            module
                .set_output_patch_enabled(self.jack_saw, self.saw.just_pressed())
                .unwrap();
            module
                .set_output_patch_enabled(self.jack_sqr, self.sqr.just_pressed())
                .unwrap();
        }
    }

    pub fn process(&mut self, block: &mut ProcessBlock<NUM_INPUTS, NUM_OUTPUTS>) {
        for i in 0..BLOCK_SIZE {
            for j in 0..CHANNELS {
                let lev = block.get_input(self.jack_level).data[i].data[j] as f32 * 0.5;
                let freq =
                    voct_to_frequency_table(block.get_input(self.jack_input).data[i].data[j]);
                let (_, tri, saw, sqr) = self.osc[j].process_approx(lev, freq);
                block.get_mut_output(self.jack_tri).data[i].data[j] = tri;
                block.get_mut_output(self.jack_saw).data[i].data[j] = saw;
                block.get_mut_output(self.jack_sqr).data[i].data[j] = sqr;
            }
        }
    }

    pub fn set_params(&mut self, _adc: &mut [u16; 8]) {}

    pub fn get_light_data(&self, update: PollUpdate<NUM_INPUTS, NUM_OUTPUTS>) -> [Srgb<u8>; 5] {
        [
            update.get_input_color(self.jack_input),
            update.get_input_color(self.jack_level),
            update.get_output_color(self.jack_tri),
            update.get_output_color(self.jack_saw),
            update.get_output_color(self.jack_sqr),
        ]
    }
}
