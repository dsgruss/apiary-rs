#![no_std]
#![no_main]

use panic_semihosting as _;
// use panic_itm as _;
// use panic_halt as _;

use apiary::{hal::{
    adc::{
        config::{AdcConfig, Clock, Continuous, SampleTime, Scan},
        Adc,
    },
    gpio::GpioExt,
    pac::{interrupt, CorePeripherals, Peripherals, USART3},
    prelude::*,
    rcc::RccExt,
    serial::Tx,
}, protocol::{HeldInputJack, HeldOutputJack}};
use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;

use core::cell::RefCell;
use core::fmt::Write;
use fugit::RateExtU32;
use heapless::String;

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

use apiary::{
    leader_election::LeaderElection,
    protocol::{Directive, Uuid, LocalState},
    ui::{Ui, UiPins},
    AudioPacket, NetworkInterface, NetworkInterfaceStorage,
};

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
    let addr = String::from("239.1.2.3");
    let mut rand_source = p.RNG.constrain(&clocks);
    let mut leader_election =
        LeaderElection::new(uuid.clone(), 0, &mut rand_source);

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

    let mut rx_ring: [RingEntry<_>; 8] = Default::default();
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

    let mut storage = NetworkInterfaceStorage::new();
    let mut network = NetworkInterface::new(&mut eth_dma, &mut storage);

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

    let mut packet = AudioPacket::new();

    let mut timer = cp.SYST.counter_us(&clocks);
    let mut time: i64 = 0;
    let mut ui_accum = 0;
    let mut send_accum = 0;
    let mut poll_accum = 0;
    let mut adc_accum = 0;
    timer.start(1.millis()).unwrap();

    loop {
        nb::block!(timer.wait()).unwrap();
        time += 1;

        let ui_start = timer.now().ticks();
        let (sw2, sw4) = ui.poll();
        let mut local_state: LocalState = Default::default();
        if sw2 {
            local_state.held_inputs.push(HeldInputJack {
                uuid: uuid.clone(),
                id: 0
            }).unwrap();
        }
        if sw4 {
            local_state.held_outputs.push(HeldOutputJack {
                uuid: uuid.clone(),
                id: 1,
                color: 48,
                addr: addr.clone(),
                port: 19991,
            }).unwrap();
        }
        leader_election.update_local_state(local_state);
        ui_accum += timer.now().ticks() - ui_start;

        let poll_start = timer.now().ticks();
        let result = network.poll(time);
        match result {
            Ok(Some(Directive::Halt { uuid })) => {
                info!("Got HALT directive: {:?}", uuid);
            }
            Ok(Some(Directive::SetInputJack {
                uuid: _,
                source,
                connection: _,
            })) => {
                network
                    .jack_connect(&source.addr, source.port, time)
                    .unwrap();
            }
            Ok(dir) => {
                if network.can_send() {
                    if let Some(resp) = leader_election.poll(dir, time) {
                        if let Err(e) = network.send(&resp) {
                            info!("UDP send error: {:?}", e);
                        }
                    }
                } else {
                    leader_election.reset(time);
                }
            }
            Err(e) => {
                // Ignore malformed packets
                info!("Error: {:?}", e);
            }
        }
        poll_accum += timer.now().ticks() - poll_start;

        let send_start = timer.now().ticks();
        if network.can_send() {
            match network.jack_poll() {
                Ok(Some(d)) => {
                    if let Err(e) = network.send_jack_data(&d) {
                        info!("Data send error: {:?}", e);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    info!("Data recv error: {:?}", e);
                }
            }
        }
        send_accum += timer.now().ticks() - send_start;

        let adc_start = timer.now().ticks();
        sample = adc.convert(&pa0, SampleTime::Cycles_84);
        for frame in &mut packet.data {
            for v in &mut frame.data {
                *v = sample as i16;
            }
        }
        adc_accum += timer.now().ticks() - adc_start;

        if time % 1000 == 0 {
            info!(
                "Average times (us): ui {}, send {}, poll {}, adc {}",
                ui_accum / 1000,
                send_accum / 1000,
                poll_accum / 1000,
                adc_accum / 1000
            );
            info!("ADC current sample: {:?}", adc.sample_to_millivolts(sample));
            info!("Election status: {:?}:{}:{}, leader is {:?}", 
                leader_election.role,
                leader_election.current_term,
                leader_election.iteration,
                leader_election.voted_for
            );

            ui_accum = 0;
            send_accum = 0;
            poll_accum = 0;
            adc_accum = 0;
        }
    }
}

#[interrupt]
fn ETH() {
    // Clear interrupt flags
    let p = unsafe { Peripherals::steal() };
    stm32_eth::eth_interrupt_handler(&p.ETHERNET_DMA);
}
