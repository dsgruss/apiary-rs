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

use smoltcp::iface::{Interface, InterfaceBuilder, NeighborCache, Routes};
use smoltcp::phy::Device;
use smoltcp::socket::{Dhcpv4Event, Dhcpv4Socket, TcpSocket, TcpSocketBuffer};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpCidr, Ipv4Address, Ipv4Cidr};

use stm32_eth::{EthPins, RingEntry};

const SRC_MAC: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

static TIME: Mutex<RefCell<u64>> = Mutex::new(RefCell::new(0));
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

#[entry]
fn main() -> ! {
    let p = Peripherals::take().unwrap();
    let mut cp = CorePeripherals::take().unwrap();

    let rcc = p.RCC.constrain();
    // HCLK must be at least 25MHz to use the ethernet peripheral
    let clocks = rcc.cfgr.sysclk(32.MHz()).hclk(32.MHz()).freeze();

    setup_systick(&mut cp.SYST);

    let gpiod = p.GPIOD.split();
    let tx_pin = gpiod.pd8.into_alternate();

    let tx = p.USART3.tx(tx_pin, 9600.bps(), &clocks).unwrap();
    cortex_m::interrupt::free(|cs| *LOGGER.tx.borrow(cs).borrow_mut() = Some(tx));
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .unwrap();
    info!("\n\nSerial debug active");

    info!("Enabling ethernet...");
    let gpioa = p.GPIOA.split();
    let gpiob = p.GPIOB.split();
    let gpioc = p.GPIOC.split();
    let gpiog = p.GPIOG.split();

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

    let ip_addr = IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0);
    let mut ip_addrs = [ip_addr];
    let mut neighbor_storage = [None; 16];
    let neighbor_cache = NeighborCache::new(&mut neighbor_storage[..]);
    let mut routes_storage = [None; 1];
    let routes = Routes::new(&mut routes_storage[..]);
    let ethernet_addr = EthernetAddress(SRC_MAC);

    let mut sockets: [_; 2] = Default::default();
    let mut iface = InterfaceBuilder::new(&mut eth_dma, &mut sockets[..])
        .hardware_addr(ethernet_addr.into())
        .ip_addrs(&mut ip_addrs[..])
        .routes(routes)
        .neighbor_cache(neighbor_cache)
        .finalize();

    let dhcp_socket = Dhcpv4Socket::new();
    let dhcp_handle = iface.add_socket(dhcp_socket);
    let mut dhcp_configured = false;

    let mut server_rx_buffer = [0; 2048];
    let mut server_tx_buffer = [0; 2048];
    let server_socket = TcpSocket::new(
        TcpSocketBuffer::new(&mut server_rx_buffer[..]),
        TcpSocketBuffer::new(&mut server_tx_buffer[..]),
    );
    let server_handle = iface.add_socket(server_socket);

    info!("Sockets created and starting main loop");
    loop {
        let time: u64 = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
        cortex_m::interrupt::free(|cs| {
            let mut eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
            *eth_pending = false;
        });
        match iface.poll(Instant::from_millis(time as i64)) {
            Ok(true) => {
                let event = iface.get_socket::<Dhcpv4Socket>(dhcp_handle).poll();
                match event {
                    None => {}
                    Some(Dhcpv4Event::Configured(config)) => {
                        info!("DHCP config acquired!");

                        info!("IP address:      {}", config.address);
                        set_ipv4_addr(&mut iface, config.address);

                        if let Some(router) = config.router {
                            info!("Default gateway: {}", router);
                            iface.routes_mut().add_default_ipv4_route(router).unwrap();
                        } else {
                            info!("Default gateway: None");
                            iface.routes_mut().remove_default_ipv4_route();
                        }

                        for (i, s) in config.dns_servers.iter().enumerate() {
                            if let Some(s) = s {
                                info!("DNS server {}:    {}", i, s);
                            }
                        }
                        dhcp_configured = true;
                    }
                    Some(Dhcpv4Event::Deconfigured) => {
                        info!("DHCP lost config!");
                        set_ipv4_addr(&mut iface, Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0));
                        iface.routes_mut().remove_default_ipv4_route();
                        dhcp_configured = false;
                    }
                }
                if !dhcp_configured {
                    continue;
                }

                let socket = iface.get_socket::<TcpSocket>(server_handle);
                if !socket.is_open() {
                    if let Err(e) = socket.listen(80) {
                        info!("TCP listen error: {:?}", e);
                    }
                }

                if socket.can_send() {
                    if let Err(e) = write!(socket, "hello\n").map(|_| {
                        socket.close();
                    }) {
                        info!("TCP send error: {:?}", e);
                    }
                }
            }
            Ok(false) => {
                // Sleep if no ethernet work is pending
                cortex_m::interrupt::free(|cs| {
                    let eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
                    if !*eth_pending {
                        asm::wfi();
                        // Awaken by interrupt
                    }
                });
            }
            Err(e) =>
            // Ignore malformed packets
            {
                info!("Error: {:?}", e);
            }
        }
    }
}

fn set_ipv4_addr<DeviceT>(iface: &mut Interface<'_, DeviceT>, cidr: Ipv4Cidr)
where
    DeviceT: for<'d> Device<'d>,
{
    iface.update_ip_addrs(|addrs| {
        let dest = addrs.iter_mut().next().unwrap();
        *dest = IpCidr::Ipv4(cidr);
    });
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
