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
    dma::{config, traits::StreamISR, MemoryToPeripheral, Stream3, StreamsTuple, Transfer},
    gpio::GpioExt,
    pac::{self, interrupt, CorePeripherals, Peripherals, DMA1, USART3},
    prelude::*,
    rcc::RccExt,
    serial::{config::DmaConfig, Config, Tx},
};

use core::cell::RefCell;
use core::fmt::{Debug, Write};
use fugit::RateExtU32;
use heapless::spsc::Queue;

use stm32_eth::{EthPins, RingEntry};

#[macro_use]
extern crate log;
use log::{Level, LevelFilter, Metadata, Record};

const LOG_BUFFER_SIZE: usize = 1024;

type SerialDma =
    Transfer<Stream3<DMA1>, 4, Tx<USART3>, MemoryToPeripheral, &'static mut [u8; LOG_BUFFER_SIZE]>;

static TRANSFER: Mutex<RefCell<Option<SerialDma>>> = Mutex::new(RefCell::new(None));
static LOG_QUEUE: Mutex<RefCell<Queue<u8, LOG_BUFFER_SIZE>>> =
    Mutex::new(RefCell::new(Queue::new()));

struct SerialLogger;

impl log::Log for SerialLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let mut s: heapless::String<LOG_BUFFER_SIZE> = Default::default();
            writeln!(s, "{} - {}", record.level(), record.args()).unwrap();
            cortex_m::interrupt::free(|cs| {
                if let Some(transfer) = TRANSFER.borrow(cs).borrow_mut().as_mut() {
                    let mut log_queue = LOG_QUEUE.borrow(cs).borrow_mut();
                    for b in s.as_bytes() {
                        if let Err(_) = log_queue.enqueue(*b) {
                            break;
                        }
                    }
                    // Safety: since the interrupt handler controls the read end of the `log_queue`,
                    // we send an empty buffer to start another transfer. This will have the effect
                    // of restarting and overwriting a transfer if one is currently in progress.
                    unsafe {
                        static mut BUFFER: [u8; LOG_BUFFER_SIZE] = [0; LOG_BUFFER_SIZE];
                        transfer.next_transfer(&mut BUFFER).unwrap();
                    }
                }
            });
        }
    }

    fn flush(&self) {}
}

static LOGGER: SerialLogger = SerialLogger {};

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

    let tx_pin = gpiod.pd8;

    let mut serial_config = Config::default();
    serial_config.dma = DmaConfig::Tx;
    let mut tx = p.USART3.tx(tx_pin, serial_config, &clocks).unwrap();
    writeln!(tx, "\n\n ☢️📶📼 v0.1.0\n\n").unwrap();

    let init_buffer =
        cortex_m::singleton!(: [u8; LOG_BUFFER_SIZE] = [70; LOG_BUFFER_SIZE]).unwrap();
    let transfer: SerialDma = Transfer::init_memory_to_peripheral(
        StreamsTuple::new(p.DMA1).3,
        tx,
        init_buffer,
        None,
        config::DmaConfig::default()
            .memory_increment(true)
            .fifo_enable(true)
            .fifo_error_interrupt(true)
            .transfer_complete_interrupt(true),
    );
    cortex_m::interrupt::free(|cs| {
        *TRANSFER.borrow(cs).borrow_mut() = Some(transfer);
    });

    // Safety: It appears that this is the preferred way to start interrupts...
    unsafe {
        cortex_m::peripheral::NVIC::unmask(pac::Interrupt::DMA1_STREAM3);
    }
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
    let mut last_stats: Times = Default::default();
    let mut curr_stats: Times = Default::default();
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

        let ui_start = cycle_timer.now();
        let (changed, sw2, sw4) = ui.poll();
        if changed {
            module.set_input_patch_enabled(0, sw2).unwrap();
            module.set_output_patch_enabled(0, sw4).unwrap();
        }
        update_time(
            (cycle_timer.now() - ui_start).to_micros() as i64,
            &mut curr_stats.ui,
        );

        let poll_start = cycle_timer.now();
        if let Err(e) = module.poll(time, |input, output| {
            output[0] = input[0];
        }) {
            info!("Data send error: {:?}", e);
        }
        update_time(
            (cycle_timer.now() - poll_start).to_micros() as i64,
            &mut curr_stats.poll,
        );

        let adc_start = cycle_timer.now();
        sample = adc.convert(&pa0, SampleTime::Cycles_84);
        for frame in &mut packet.data {
            for v in &mut frame.data {
                *v = 0 as i16;
            }
        }
        update_time(
            (cycle_timer.now() - adc_start).to_micros() as i64,
            &mut curr_stats.adc,
        );
        if time % 1000 == 0 {
            info!("total, max (us): {:?}", last_stats);
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
            last_stats = curr_stats;
            curr_stats = Default::default();
        }
        update_time(
            (cycle_timer.now() - start).to_micros() as i64,
            &mut curr_stats.total,
        );
        cycle_time += (cycle_timer.now() - start).to_millis() as i64;
    }
}

#[derive(Default)]
struct Times {
    ui: (i64, i64),
    send: (i64, i64),
    poll: (i64, i64),
    adc: (i64, i64),
    total: (i64, i64),
}

impl Debug for Times {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Times")
            .field("ui", &(self.ui.0 / 1000, self.ui.1))
            .field("send", &(self.send.0 / 1000, self.send.1))
            .field("poll", &(self.poll.0 / 1000, self.poll.1))
            .field("adc", &(self.adc.0 / 1000, self.adc.1))
            .field("total", &(self.total.0 / 1000, self.total.1))
            .finish()
    }
}

fn update_time(micros: i64, store: &mut (i64, i64)) {
    store.0 += micros;
    if micros > store.1 {
        store.1 = micros;
    }
}

#[interrupt]
fn DMA1_STREAM3() {
    cortex_m::interrupt::free(|cs| {
        if let Some(transfer) = TRANSFER.borrow(cs).borrow_mut().as_mut() {
            if Stream3::<pac::DMA1>::get_fifo_error_flag() {
                transfer.clear_fifo_error_interrupt();
            }
            if Stream3::<pac::DMA1>::get_transfer_complete_flag() {
                transfer.clear_transfer_complete_interrupt();
                let mut log_queue = LOG_QUEUE.borrow(cs).borrow_mut();
                if !log_queue.is_empty() {
                    // Safety: This shouldn't be necessary in the long run: `next_transfer` returns
                    // the reference to the old buffer, so ideally we would swap them here rather
                    // than relying on the single reference. This method found in the spi_dma
                    // example in the hal.
                    unsafe {
                        static mut BUFFER: [u8; LOG_BUFFER_SIZE] = [0; LOG_BUFFER_SIZE];
                        BUFFER = [0; LOG_BUFFER_SIZE];
                        for b in BUFFER.iter_mut() {
                            match log_queue.dequeue() {
                                Some(val) => *b = val,
                                None => break,
                            }
                        }
                        transfer.next_transfer(&mut BUFFER).unwrap();
                    }
                }
            }
        }
    });
}
