#![no_std]
#![no_main]

use panic_semihosting as _;
// use panic_itm as _;

use cortex_m::asm;
use cortex_m_rt::{entry, exception};
use stm32_eth::{
    hal::gpio::GpioExt,
    hal::prelude::*,
    hal::rcc::RccExt,
    hal::serial::Tx,
    stm32::{interrupt, CorePeripherals, Peripherals, SYST, USART3},
};

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;

use core::fmt::Write;

use fugit::RateExtU32;

use stm32_eth::{EthPins, RingEntry};

static TIME: Mutex<RefCell<i64>> = Mutex::new(RefCell::new(0));
static ETH_PENDING: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(false));

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
    let mut cp = CorePeripherals::take().unwrap();

    let rcc = p.RCC.constrain();
    // HCLK must be at least 25MHz to use the ethernet peripheral
    let clocks = rcc.cfgr.sysclk(32.MHz()).hclk(32.MHz()).freeze();

    setup_systick(&mut cp.SYST);

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
    let mut tx_ring: [RingEntry<_>; 2] = Default::default();
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

    loop {
        let time: i64 = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
        cortex_m::interrupt::free(|cs| {
            let mut eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
            *eth_pending = false;
        });

        if ui.poll() {
            if network.can_send() {
                info!("=> HALT");
                if let Err(e) = network.send(&halt) {
                    info!("UDP send error: {:?}", e);
                }
            }
        }

        match network.poll(time) {
            Ok(Some(directive)) => {
                info!("Got directive: {:?}", directive);
            }
            Ok(None) => {
                // Sleep if no ethernet work is pending
                cortex_m::interrupt::free(|cs| {
                    let eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
                    if !*eth_pending {
                        asm::wfi();
                        // Awaken by interrupt
                    }
                });
            }
            Err(e) => {
                // Ignore malformed packets
                info!("Error: {:?}", e);
            }
        }
    }
}

fn setup_systick(syst: &mut SYST) {
    syst.set_reload(SYST::get_ticks_per_10ms() / 10);
    syst.enable_counter();
    syst.enable_interrupt();
}

#[exception]
fn SysTick() {
    cortex_m::interrupt::free(|cs| {
        let mut time = TIME.borrow(cs).borrow_mut();
        *time += 1;
    })
}

#[interrupt]
fn ETH() {
    cortex_m::interrupt::free(|cs| {
        let mut eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
        *eth_pending = true;
    });

    // Clear interrupt flags
    let p = unsafe { Peripherals::steal() };
    stm32_eth::eth_interrupt_handler(&p.ETHERNET_DMA);
}
