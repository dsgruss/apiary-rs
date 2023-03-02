#![no_std]

use panic_semihosting as _;

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

use core::{fmt::Debug, fmt::Write, hash::Hash};
use fugit::RateExtU32;
use hash32::{FnvHasher, Hasher};

use stm32_eth::{EthPins, RingEntry};

#[macro_use]
extern crate log;

use apiary_core::{socket_smoltcp::SmoltcpInterface, Module, Uuid};

mod filter;
use filter as engine;
use filter::{Filter, FilterPins};
// mod oscillator;
// use oscillator as engine;
// use oscillator::{Oscillator, OscillatorPins};
// mod envelope;
// use envelope as engine;
// use envelope::{Envelope, EnvelopePins};

pub mod apa102;
use apa102::Apa102;

mod serial_logger;
mod ui;

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
    apa.set_intensity(8);

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
    write!(uuid, "hardware:{}:{:#08x}", engine::NAME, val).unwrap();

    let mut storage = Default::default();
    let mut module: Module<_, _, { engine::NUM_INPUTS }, { engine::NUM_OUTPUTS }> = Module::new(
        SmoltcpInterface::<
            _,
            { engine::NUM_INPUTS },
            { engine::NUM_OUTPUTS },
            { engine::NUM_INPUTS + engine::NUM_OUTPUTS + 1 },
        >::new(&mut eth_dma, mac, &mut storage),
        rand_source,
        uuid.clone(),
        engine::COLOR,
        0,
    );

    let filter_pins = FilterPins {
        input: gpioc.pc8,
        key_track: gpioc.pc9,
        contour: gpiod.pd12,
        output: gpiod.pd13,
    };
    let mut en = Filter::new(filter_pins, &mut module);
    // let oscillator_pins = OscillatorPins {
    //     input: gpioc.pc7,
    //     level: gpioc.pc8,
    //     tri: gpioc.pc9,
    //     saw: gpiod.pd12,
    //     sqr: gpiod.pd13,
    // };
    // let mut en = Oscillator::new(oscillator_pins, &mut module);
    // let envelope_pins = EnvelopePins {
    //     gate: gpiod.pd12,
    //     level: gpiod.pd13,
    // };
    // let mut en = Envelope::new(envelope_pins, &mut module);

    info!("Sockets created");

    // ADC3 GPIO Configuration
    // PA0/WKUP ------> ADC3_IN0
    // PF7      ------> ADC3_IN5
    // PF8      ------> ADC3_IN6
    // PF9      ------> ADC3_IN7
    // PF10     ------> ADC3_IN8
    // PF3      ------> ADC3_IN9
    // PF4      ------> ADC3_IN14
    // PF5      ------> ADC3_IN15

    let adc_config = AdcConfig::default()
        .dma(Dma::Continuous)
        .clock(Clock::Pclk2_div_8)
        .scan(Scan::Enabled)
        .continuous(Continuous::Single);
    let adc_dma_config = config::DmaConfig::default()
        .double_buffer(false)
        .memory_increment(true);

    let mut adc = Adc::adc3(p.ADC3, true, adc_config);
    let st = SampleTime::Cycles_480;
    adc.configure_channel(&gpioa.pa0.into_analog(), Sequence::One, st);
    adc.configure_channel(&gpiof.pf7.into_analog(), Sequence::Two, st);
    adc.configure_channel(&gpiof.pf8.into_analog(), Sequence::Three, st);
    adc.configure_channel(&gpiof.pf9.into_analog(), Sequence::Four, st);
    adc.configure_channel(&gpiof.pf10.into_analog(), Sequence::Five, st);
    adc.configure_channel(&gpiof.pf3.into_analog(), Sequence::Six, st);
    adc.configure_channel(&gpiof.pf4.into_analog(), Sequence::Seven, st);
    adc.configure_channel(&gpiof.pf5.into_analog(), Sequence::Eight, st);

    let init_adc_buffer = cortex_m::singleton!(: [u16; 8] = [0; 8]).unwrap();
    let mut adc_transfer = Transfer::init_peripheral_to_memory(
        StreamsTuple::new(p.DMA2).0,
        adc,
        init_adc_buffer,
        None,
        adc_dma_config,
    );

    adc_transfer.start(|adc| adc.start_conversion());
    let mut adc_buffer = cortex_m::singleton!(: [u16; 8] = [0; 8]).unwrap();
    adc_buffer = adc_transfer.next_transfer(adc_buffer).unwrap().0;
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
        en.poll_ui(&mut module);
        curr_stats.ui.toc(cycle_timer.now());

        curr_stats.poll.tic(cycle_timer.now());
        match module.poll(time, |block| {
            curr_stats.process.tic(cycle_timer.now());
            en.process(block);
            curr_stats.process.toc(cycle_timer.now());
        }) {
            Ok(update) => {
                let light_data = en.get_light_data(update);
                apa.write(light_data.iter().cloned()).unwrap();
            }
            Err(e) => info!("Data send error: {:?}", e),
        }
        curr_stats.poll.toc(cycle_timer.now());

        curr_stats.adc.tic(cycle_timer.now());
        adc_transfer.start(|adc| adc.start_conversion());
        adc_buffer = adc_transfer.next_transfer(adc_buffer).unwrap().0;
        en.set_params(adc_buffer);
        curr_stats.adc.toc(cycle_timer.now());

        if time % 1000 == 0 {
            info!("total, max (us): {:?}", last_stats);
            info!("ADC current sample: {:?}", adc_buffer);
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
