#![no_std]

use hash32::{FnvHasher, Hasher};
use panic_semihosting as _;
// use panic_itm as _;
// use panic_halt as _;

use stm32f4xx_hal::{
    adc::{
        config::{AdcConfig, Clock, Continuous, Dma, SampleTime, Scan, Sequence},
        Adc,
    },
    dma::{config, StreamsTuple, Transfer},
    gpio::{GpioExt, NoPin},
    pac::{CorePeripherals, Peripherals},
    prelude::*,
    rcc::RccExt,
    signature::Uid,
    spi::Spi,
};

use core::iter::zip;
use core::{fmt::Debug, fmt::Write, hash::Hash};
use fugit::RateExtU32;
use itertools::izip;
use libm::{log10f, powf};
use palette::Srgb;

use stm32_eth::{EthPins, RingEntry};

#[macro_use]
extern crate log;

use apiary_core::{
    dsp::LinearTrap, socket_smoltcp::SmoltcpInterface, softclip, voct_to_freq_scale, AudioPacket,
    Module, Uuid, CHANNELS,
};

pub mod filter;
use filter::{Ui, UiPins};

pub mod apa102;
use apa102::Apa102;

mod serial_logger;

const NUM_INPUTS: usize = 3;
const NUM_OUTPUTS: usize = 1;

pub fn start() -> ! {
    let p = Peripherals::take().unwrap();
    let cp = CorePeripherals::take().unwrap();

    let rcc = p.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.MHz())
        .sysclk(168.MHz())
        .require_pll48clk()
        .freeze();

    let gpioa = p.GPIOA.split();
    let gpiob = p.GPIOB.split();
    let gpioc = p.GPIOC.split();
    let gpiod = p.GPIOD.split();
    let gpiof = p.GPIOF.split();
    let gpiog = p.GPIOG.split();

    serial_logger::init(gpiod.pd8, p.USART3, p.DMA1, &clocks);

    let rand_source = p.RNG.constrain(&clocks);

    let sck = gpioc.pc10.into_alternate();
    let miso = NoPin;
    let mosi = gpioc.pc12.into_alternate();

    let spi = Spi::new(p.SPI3, (sck, miso, mosi), apa102::MODE, 32.MHz(), &clocks);
    let mut apa = Apa102::new(spi).pixel_order(apa102::PixelOrder::RBG);
    let mut light_data: [Srgb<u8>; NUM_INPUTS + NUM_OUTPUTS] =
        [Srgb::new(255, 255, 255); NUM_INPUTS + NUM_OUTPUTS];

    let ui_pins = UiPins {
        input: gpioc.pc8,
        key_track: gpioc.pc9,
        contour: gpiod.pd12,
        output: gpiod.pd13,
    };
    let mut ui = Ui::new(ui_pins);

    info!("Enabling ethernet...");
    let eth_pins = EthPins {
        ref_clk: gpioa.pa1,
        crs: gpioa.pa7,
        tx_en: gpiog.pg11,
        tx_d0: gpiog.pg13,
        tx_d1: gpiob.pb13,
        rx_d0: gpioc.pc4,
        rx_d1: gpioc.pc5,
    };

    let mut rx_ring: [RingEntry<_>; 16] = Default::default();
    let mut tx_ring: [RingEntry<_>; 16] = Default::default();
    let (mut eth_dma, _eth_mac) = stm32_eth::new(
        p.ETHERNET_MAC,
        p.ETHERNET_MMC,
        p.ETHERNET_DMA,
        &mut rx_ring[..],
        &mut tx_ring[..],
        clocks,
        eth_pins,
    )
    .unwrap();

    // Allow some time for the interface to come up before starting the IP stack
    let mut cycle_timer = p.TIM5.counter_us(&clocks);
    cycle_timer.start(2.secs()).unwrap();
    nb::block!(cycle_timer.wait()).unwrap();

    // Derive the mac address and module id from the unique device id
    let mut s = FnvHasher::default();
    Uid::get().hash(&mut s);
    let val = s.finish32();
    let bval = val.to_ne_bytes();
    let mac = [0x00, 0x00, bval[0], bval[1], bval[2], bval[3]];

    info!("Setting mac address to: {:?}", mac);

    let mut uuid = Uuid::default();
    write!(uuid, "hardware:filter:{:#08x}", val).unwrap();

    let mut storage = Default::default();
    let mut module: Module<_, _, NUM_INPUTS, NUM_OUTPUTS> = Module::new(
        SmoltcpInterface::<_, NUM_INPUTS, NUM_OUTPUTS, { NUM_INPUTS + NUM_OUTPUTS + 1 }>::new(
            &mut eth_dma,
            mac,
            &mut storage,
        ),
        rand_source,
        uuid.clone(),
        220,
        0,
    );

    let jack_input = module.add_input_jack().unwrap();
    let jack_key_track = module.add_input_jack().unwrap();
    let jack_contour = module.add_input_jack().unwrap();

    let jack_output = module.add_output_jack().unwrap();

    info!("Sockets created");

    // ADC3 GPIO Configuration
    // PF3     ------> ADC3_IN9
    // PF4     ------> ADC3_IN14
    // PF5     ------> ADC3_IN15
    // PF7     ------> ADC3_IN5
    // PF8     ------> ADC3_IN6
    // PF9     ------> ADC3_IN7
    // PF10     ------> ADC3_IN8
    // PA0/WKUP     ------> ADC3_IN0

    let adc_config = AdcConfig::default()
        .dma(Dma::Continuous)
        .clock(Clock::Pclk2_div_8)
        .scan(Scan::Enabled)
        .continuous(Continuous::Single);
    let adc_dma_config = config::DmaConfig::default()
        .double_buffer(false)
        .memory_increment(true);

    let mut adc = Adc::adc3(p.ADC3, true, adc_config);
    adc.configure_channel(
        &gpioa.pa0.into_analog(),
        Sequence::One,
        SampleTime::Cycles_480,
    );
    adc.configure_channel(
        &gpiof.pf7.into_analog(),
        Sequence::Two,
        SampleTime::Cycles_480,
    );
    adc.configure_channel(
        &gpiof.pf8.into_analog(),
        Sequence::Three,
        SampleTime::Cycles_480,
    );
    adc.configure_channel(
        &gpiof.pf9.into_analog(),
        Sequence::Four,
        SampleTime::Cycles_480,
    );
    let init_adc_buffer = cortex_m::singleton!(: [u16; 4] = [0; 4]).unwrap();
    let mut adc_transfer = Transfer::init_peripheral_to_memory(
        StreamsTuple::new(p.DMA2).0,
        adc,
        init_adc_buffer,
        None,
        adc_dma_config,
    );
    adc_transfer.start(|adc| adc.start_conversion());
    let mut adc_buffer = cortex_m::singleton!(: [u16; 4] = [0; 4]).unwrap();
    adc_buffer = adc_transfer.next_transfer(adc_buffer).unwrap().0;
    let mut params: [f32; 3] = [0.0; 3];
    let mut filters: [LinearTrap; CHANNELS] = Default::default();
    info!("ADC current sample: {:?}", adc_buffer);

    info!("Starting main loop");

    let mut timer = cp.SYST.counter_us(&clocks);
    let mut time: i64 = 0;
    let mut cycle_time: i64 = 0;
    let mut last_stats: Stats = Default::default();
    let mut curr_stats: Stats = Default::default();
    timer.start(1.millis()).unwrap();
    cycle_timer.start(100.millis()).unwrap();

    loop {
        // We need to have each update occur as close as possible to the 1 ms mark, however (at
        // least with the serial monitor on), some cycles will end up taking longer. Here, an
        // additional timer is used to "catch up" on missed cycles.
        if cycle_time < time {
            nb::block!(timer.wait()).unwrap();
            cycle_time += 1
        }
        cycle_timer.start(100.millis()).unwrap();
        curr_stats.total.tic(cycle_timer.now());
        let start = cycle_timer.now();
        time += 1;

        curr_stats.ui.tic(cycle_timer.now());
        let res = ui.poll();
        if res.changed {
            module
                .set_input_patch_enabled(jack_input, res.input_pressed)
                .unwrap();
            module
                .set_input_patch_enabled(jack_key_track, res.key_track_pressed)
                .unwrap();
            module
                .set_input_patch_enabled(jack_contour, res.contour_pressed)
                .unwrap();
            module
                .set_output_patch_enabled(jack_output, res.output_pressed)
                .unwrap();
        }
        curr_stats.ui.toc(cycle_timer.now());

        curr_stats.poll.tic(cycle_timer.now());
        match module.poll(time, |block| {
            curr_stats.process.tic(cycle_timer.now());
            // Processing time is too slow to do this every audio frame...
            for i in 0..CHANNELS {
                filters[i].set_params(
                    params[0]
                        * voct_to_freq_scale(
                            block.get_input(jack_key_track).data[0].data[i] as f32
                                + block.get_input(jack_contour).data[0].data[i] as f32
                                    / i16::MAX as f32
                                    * params[2]
                                    * 512.0
                                    * 12.0
                                    * 4.0,
                        ),
                    params[1],
                );
            }
            let mut output: AudioPacket = Default::default();
            for (fin, fout) in zip(block.get_input(jack_input).data, output.data.iter_mut()) {
                for (iin, iout, filter) in izip!(fin.data, fout.data.iter_mut(), filters.iter_mut())
                {
                    *iout = (softclip(filter.process(iin as f32 / i16::MAX as f32))
                        * i16::MAX as f32) as i16;
                }
            }
            block.set_output(jack_output, output);
            curr_stats.process.toc(cycle_timer.now());
        }) {
            Ok(update) => {
                light_data[0] = update.get_input_color(jack_key_track);
                light_data[1] = update.get_input_color(jack_contour);
                light_data[2] = update.get_input_color(jack_input);
                light_data[3] = update.get_output_color(jack_output);
                // let active = (time / 100) % 4;
                // for i in 0..4 {
                //     light_data[i] = Srgb::new(0, 0, 0);
                // }
                // light_data[active as usize] = Srgb::new(255, 255, 255);
                apa.set_intensity(8);
                apa.write(light_data.iter().cloned()).unwrap();
            }
            Err(e) => info!("Data send error: {:?}", e),
        }
        curr_stats.poll.toc(cycle_timer.now());

        curr_stats.adc.tic(cycle_timer.now());
        adc_transfer.start(|adc| adc.start_conversion());
        adc_buffer = adc_transfer.next_transfer(adc_buffer).unwrap().0;
        params[0] += 0.01
            * (20.0
                * powf(
                    10.0,
                    (adc_buffer[0] as f32 / 4096.0) * log10f(8000.0 / 20.0),
                )
                - params[0]);
        params[1] = powf(adc_buffer[1] as f32 / 4096.0, 2.0) * 10.0;
        params[2] = adc_buffer[2] as f32 / 4096.0;
        curr_stats.adc.toc(cycle_timer.now());

        if time % 1000 == 0 {
            info!("total, max (us): {:?}", last_stats);
            info!("ADC current sample: {:?}, Params: {:?}", adc_buffer, params);
            /*
            info!(
                "Election status: {:?}:{}:{}, leader is {:?}",
                leader_election.role,
                leader_election.current_term,
                leader_election.iteration,
                leader_election.voted_for
            );
            */
            last_stats = curr_stats;
            curr_stats = Default::default();
        }
        curr_stats.total.toc(cycle_timer.now());
        cycle_time += (cycle_timer.now() - start).to_millis() as i64;
    }
}

#[derive(Default)]
struct Stats {
    ui: StatTimer,
    process: StatTimer,
    poll: StatTimer,
    adc: StatTimer,
    total: StatTimer,
}

#[derive(Default)]
struct StatTimer {
    begin: Option<fugit::Instant<u32, 1_u32, 1000000_u32>>,
    total: i64,
    max: i64,
}

impl StatTimer {
    fn tic(&mut self, time: fugit::Instant<u32, 1_u32, 1000000_u32>) {
        self.begin = Some(time);
    }

    fn toc(&mut self, time: fugit::Instant<u32, 1_u32, 1000000_u32>) {
        if let Some(begin) = self.begin {
            let diff = (time - begin).to_micros() as i64;
            self.total += diff;
            if diff > self.max {
                self.max = diff;
            }
        }
    }
}

impl Debug for Stats {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Stats")
            .field("ui", &(self.ui.total / 1000, self.ui.max))
            .field("process", &(self.process.total / 1000, self.process.max))
            .field("poll", &(self.poll.total / 1000, self.poll.max))
            .field("adc", &(self.adc.total / 1000, self.adc.max))
            .field("total", &(self.total.total / 1000, self.total.max))
            .finish()
    }
}
