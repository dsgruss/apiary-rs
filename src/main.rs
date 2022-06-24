#![no_std]
#![no_main]

use panic_semihosting as _;
// use panic_itm as _;

use cortex_m_rt::entry;
use stm32_eth::{
    hal::gpio::GpioExt,
    hal::prelude::*,
    hal::rcc::RccExt,
    hal::serial::Tx,
    stm32::{interrupt, CorePeripherals, Peripherals, USART3},
};

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;

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

use apiary::{protocol::Directive, ui::Ui, ui::UiPins, NetworkInterface, NetworkInterfaceStorage};

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
    writeln!(tx, "\n\n").unwrap();
    cortex_m::interrupt::free(|cs| *LOGGER.tx.borrow(cs).borrow_mut() = Some(tx));
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .unwrap();
    info!("Serial debug active");

    // let mut rand_source = p.RNG.constrain(&clocks);

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

    info!("Sockets created and starting main loop");

    let halt = Directive::Halt { uuid: "GLOBAL" };
    let data = [7_u8; 2 * 8 * 48];

    let mut timer = cp.SYST.counter_us(&clocks);
    let mut time: i64 = 0;
    let mut ui_accum = 0;
    let mut send_accum = 0;
    let mut poll_accum = 0;
    timer.start(1.millis()).unwrap();

    loop {
        nb::block!(timer.wait()).unwrap();
        time += 1;

        let ui_start = timer.now().ticks();
        if ui.poll() {
            if network.can_send() {
                info!("=> HALT");
                if let Err(e) = network.send(&halt) {
                    info!("UDP send error: {:?}", e);
                }
            }
        }
        ui_accum += timer.now().ticks() - ui_start;

        let send_start = timer.now().ticks();
        if network.can_send() {
            if let Err(e) = network.send_jack_data(&data) {
                info!("Data send error: {:?}", e);
            }
        }
        send_accum += timer.now().ticks() - send_start;

        let poll_start = timer.now().ticks();
        match network.poll(time) {
            Ok(Some(directive)) => {
                info!("Got directive: {:?}", directive);
            }
            Ok(None) => {}
            Err(e) => {
                // Ignore malformed packets
                info!("Error: {:?}", e);
            }
        }
        poll_accum += timer.now().ticks() - poll_start;

        if time % 1000 == 0 {
            info!(
                "Average times (us): ui {}, send {}, poll {}",
                ui_accum / 1000,
                send_accum / 1000,
                poll_accum / 1000
            );
            ui_accum = 0;
            send_accum = 0;
            poll_accum = 0;
        }
    }
}

#[interrupt]
fn ETH() {
    // Clear interrupt flags
    let p = unsafe { Peripherals::steal() };
    stm32_eth::eth_interrupt_handler(&p.ETHERNET_DMA);
}
