use apiary_core::{
    dsp::oscillators::WtOscillator, AudioPacket, InputJackHandle, Module, Network,
    OutputJackHandle, PollUpdate, ProcessBlock, CHANNELS, voct_to_frequency_table,
};
use itertools::izip;
use palette::Srgb;
use rand_core::RngCore;
use stm32f4xx_hal::gpio;

use crate::ui::Switch;

pub const NUM_INPUTS: usize = 1;
pub const NUM_OUTPUTS: usize = 4;
pub const COLOR: u16 = 125;
pub const NAME: &str = "oscillator";

pub struct OscillatorPins {
    pub input: gpio::Pin<'C', 7>,
    pub sin: gpio::Pin<'C', 8>,
    pub tri: gpio::Pin<'C', 9>,
    pub saw: gpio::Pin<'D', 12>,
    pub sqr: gpio::Pin<'D', 13>,
}

pub struct Oscillator {
    input: Switch<'C', 7>,
    sin: Switch<'C', 8>,
    tri: Switch<'C', 9>,
    saw: Switch<'D', 12>,
    sqr: Switch<'D', 13>,
    osc: [WtOscillator; CHANNELS],
    jack_input: InputJackHandle,
    jack_sin: OutputJackHandle,
    jack_tri: OutputJackHandle,
    jack_saw: OutputJackHandle,
    jack_sqr: OutputJackHandle,
    params: [f32; 3],
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
            sin: Switch::new(pins.sin),
            tri: Switch::new(pins.tri),
            saw: Switch::new(pins.saw),
            sqr: Switch::new(pins.sqr),
            osc: Default::default(),
            jack_input: module.add_input_jack().unwrap(),
            jack_sin: module.add_output_jack().unwrap(),
            jack_tri: module.add_output_jack().unwrap(),
            jack_saw: module.add_output_jack().unwrap(),
            jack_sqr: module.add_output_jack().unwrap(),
            params: [0.0; 3],
        }
    }

    pub fn poll_ui<T, R>(&mut self, module: &mut Module<T, R, NUM_INPUTS, NUM_OUTPUTS>)
    where
        T: Network<NUM_INPUTS, NUM_OUTPUTS>,
        R: RngCore,
    {
        self.input.debounce();
        self.sin.debounce();
        self.tri.debounce();
        self.saw.debounce();
        self.sqr.debounce();

        if self.input.changed()
            || self.sin.changed()
            || self.tri.changed()
            || self.saw.changed()
            || self.sqr.changed()
        {
            module
                .set_input_patch_enabled(self.jack_input, self.input.just_pressed())
                .unwrap();
            module
                .set_output_patch_enabled(self.jack_sin, self.sin.just_pressed())
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
        let mut sin_out = AudioPacket::default();
        let mut tri_out = AudioPacket::default();
        let mut saw_out = AudioPacket::default();
        let mut sqr_out = AudioPacket::default();
        let a = 16000.0;
        // voct_to_frequency_table(din.data[0]);
        let freq_start = block.get_input(self.jack_input).data[0].data.map(|x| voct_to_frequency_table(x));
        for (dsin, dtri, dsaw, dsqr) in izip!(
            sin_out.data.iter_mut(),
            tri_out.data.iter_mut(),
            saw_out.data.iter_mut(),
            sqr_out.data.iter_mut()
        ) {
            for (freq, csin, ctri, csaw, csqr, osc) in izip!(
                freq_start,
                dsin.data.iter_mut(),
                dtri.data.iter_mut(),
                dsaw.data.iter_mut(),
                dsqr.data.iter_mut(),
                self.osc.iter_mut()
            ) {
                let (sin, tri, saw, sqr) = osc.process_approx(a, freq);
                *csin = sin;
                *ctri = tri;
                *csaw = saw;
                *csqr = sqr;
            }
        }
        // *block.get_mut_output(self.jack_sin) = sin_out;
        // *block.get_mut_output(self.jack_tri) = tri_out;
        *block.get_mut_output(self.jack_saw) = saw_out;
        *block.get_mut_output(self.jack_sqr) = sqr_out;
        // block.set_output(self.jack_sin, sin_out);
        // block.set_output(self.jack_tri, tri_out);
        // block.set_output(self.jack_saw, saw_out);
        // block.set_output(self.jack_sqr, sqr_out);
    }

    pub fn set_params(&mut self, adc: &mut [u16; 8]) {}

    pub fn get_light_data(&self, update: PollUpdate<NUM_INPUTS, NUM_OUTPUTS>) -> [Srgb<u8>; 5] {
        [
            update.get_input_color(self.jack_input),
            update.get_output_color(self.jack_sin),
            update.get_output_color(self.jack_tri),
            update.get_output_color(self.jack_saw),
            update.get_output_color(self.jack_sqr),
        ]
    }
}
