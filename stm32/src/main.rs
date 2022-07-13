#![no_std]
#![no_main]

use panic_semihosting as _;
// use panic_itm as _;
// use panic_halt as _;

use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;
use stm32f4xx_hal::{
    adc::{
        config::{AdcConfig, Clock, Continuous, SampleTime, Scan},
        Adc,
    },
    gpio::GpioExt,
    pac::{interrupt, CorePeripherals, Peripherals, USART3},
    prelude::*,
    rcc::RccExt,
    serial::Tx,
};

use core::cell::RefCell;
use core::fmt::Write;
use fugit::RateExtU32;

use stm32_eth::{EthPins, RingEntry};

#[macro_use]
extern crate log;
use log::{Level, LevelFilter, Metadata, Record};

type SerialTx = Tx<USART3, u8>;

struct SerialLogger {
    tx: Mutex<RefCell<Option<SerialTx>>>,
}

impl SerialLogger {
    const fn new() -> SerialLogger {
        SerialLogger {
            tx: Mutex::new(RefCell::new(None)),
        }
    }
}

impl log::Log for SerialLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            cortex_m::interrupt::free(|cs| {
                if let Some(tx) = self.tx.borrow(cs).borrow_mut().as_mut() {
                    writeln!(*tx, "{} - {}", record.level(), record.args()).unwrap();
                }
            });
        }
    }

    fn flush(&self) {}
}

static LOGGER: SerialLogger = SerialLogger::new();

use apiary_core::{socket_smoltcp::SmoltcpInterface, AudioPacket, Module, Uuid};

use apiary::{Ui, UiPins};

#[entry]
fn main() -> ! {
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
    let gpiog = p.GPIOG.split();

    let tx_pin = gpiod.pd8.into_alternate();

    let mut tx = p.USART3.tx(tx_pin, 115_200_u32.bps(), &clocks).unwrap();
    writeln!(tx, "\n\n ☢️📶📼 v0.1.0\n\n").unwrap();
    cortex_m::interrupt::free(|cs| *LOGGER.tx.borrow(cs).borrow_mut() = Some(tx));
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .unwrap();
    info!("Serial debug active");

    let uuid = Uuid::from("hardware");
    let rand_source = p.RNG.constrain(&clocks);

    let ui_pins = UiPins {
        sw_sig2: gpiod.pd12,
        sw_sig4: gpioc.pc8,
        sw_light2: gpiod.pd13,
        sw_light4: gpioc.pc9,
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

    eth_dma.enable_interrupt();

    let mut storage = Default::default();
    let mut module: Module<_, _, 1, 1> = Module::new(
        SmoltcpInterface::<_, 1, 1, 3>::new(&mut eth_dma, &mut storage),
        rand_source,
        uuid.clone(),
        0,
    );

    info!("Sockets created");

    let adc_config = AdcConfig::default()
        .clock(Clock::Pclk2_div_8)
        .scan(Scan::Enabled)
        .continuous(Continuous::Single);

    let mut adc = Adc::adc3(p.ADC3, true, adc_config);
    let pa0 = gpioa.pa0.into_analog();
    let mut sample = adc.convert(&pa0, SampleTime::Cycles_480);
    let millivolts = adc.sample_to_millivolts(sample);
    info!("ADC current sample: {:?}", millivolts);

    info!("Starting main loop");

    let mut packet: AudioPacket = Default::default();

    let mut timer = cp.SYST.counter_us(&clocks);
    let mut cycle_timer = p.TIM5.counter_us(&clocks);
    let mut time: i64 = 0;
    let mut cycle_time: i64 = 0;
    let mut ui_accum = 0;
    let mut send_accum = 0;
    let mut poll_accum = 0;
    let mut adc_accum = 0;
    let mut total_accum: u64 = 0;
    let mut total_max = 0;
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
        let start = cycle_timer.now();
        time += 1;

        // let ui_start = timer.now();
        let (changed, sw2, sw4) = ui.poll();
        if changed {
            module.set_input_patch_enabled(0, sw2).unwrap();
            module.set_output_patch_enabled(0, sw4).unwrap();
        }
        // ui_accum += (timer.now() - ui_start).to_micros();

        // let poll_start = timer.now();
        if let Err(e) = module.poll(time, |input, output| {
            output[0] = input[0];
        }) {
            info!("Data send error: {:?}", e);
        }
        // let poll_len = (timer.now() - poll_start).to_micros();
        // poll_accum += poll_len;
        // if poll_len > 500 {
        //     info!("Long poll detected: {:?}", poll_len);
        // }

        // let send_start = timer.now();
        // send_accum += (timer.now() - send_start).to_micros();

        // let adc_start = timer.now();
        sample = adc.convert(&pa0, SampleTime::Cycles_84);
        for frame in &mut packet.data {
            for v in &mut frame.data {
                *v = 0 as i16;
            }
        }
        // adc_accum += (timer.now() - adc_start).to_micros();
        let end = (cycle_timer.now() - start).to_micros();
        total_accum += end as u64;
        if end > total_max {
            total_max = end;
        }
        if time % 1000 == 0 {
            info!(
                "Average times (us): ui {}, send {}, poll {}, adc {}, total {}/{}",
                ui_accum / 1000,
                send_accum / 1000,
                poll_accum / 1000,
                adc_accum / 1000,
                total_accum / 1000,
                total_max
            );
            info!("ADC current sample: {:?}", adc.sample_to_millivolts(sample));
            /*
            info!(
                "Election status: {:?}:{}:{}, leader is {:?}",
                leader_election.role,
                leader_election.current_term,
                leader_election.iteration,
                leader_election.voted_for
            );
            */

            ui_accum = 0;
            send_accum = 0;
            poll_accum = 0;
            adc_accum = 0;
            total_accum = 0;
            total_max = 0;
        }
        cycle_time += (cycle_timer.now() - start).to_millis() as i64;
    }
}

#[interrupt]
fn ETH() {
    // Clear interrupt flags
    let p = unsafe { Peripherals::steal() };
    stm32_eth::eth_interrupt_handler(&p.ETHERNET_DMA);
}
