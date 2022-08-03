use core::{cell::RefCell, fmt::Write};
use cortex_m::interrupt::Mutex;
use heapless::spsc::Queue;
use stm32f4xx_hal::{
    dma::{config, traits::StreamISR, MemoryToPeripheral, Stream3, StreamsTuple, Transfer},
    gpio::Pin,
    interrupt,
    pac::{self, DMA1, USART3},
    prelude::*,
    rcc::Clocks,
    serial::{config::DmaConfig, Config, Tx},
};

use log::{Level, LevelFilter, Metadata, Record};

const LOG_BUFFER_SIZE: usize = 1024;

type SerialDma =
    Transfer<Stream3<DMA1>, 4, Tx<USART3>, MemoryToPeripheral, &'static mut [u8; LOG_BUFFER_SIZE]>;

static TRANSFER: Mutex<RefCell<Option<SerialDma>>> = Mutex::new(RefCell::new(None));
static LOG_QUEUE: Mutex<RefCell<Queue<u8, LOG_BUFFER_SIZE>>> =
    Mutex::new(RefCell::new(Queue::new()));
static TRANSFER_IDLE: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(true));

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
                    // Currently, the only way I can think to get back to the interrupt handler
                    // without unsafe code is to end a transfer with all null chars, then restart
                    // the transfer and resend the group of null chars...
                    let mut transfer_idle = TRANSFER_IDLE.borrow(cs).borrow_mut();
                    if *transfer_idle {
                        *transfer_idle = false;
                        transfer.start(|_| {});
                    }
                }
            });
        }
    }

    fn flush(&self) {}
}

static LOGGER: SerialLogger = SerialLogger {};

pub fn init(tx_pin: Pin<'D', 8>, usart3: USART3, dma1: DMA1, clocks: &Clocks) {
    let mut serial_config = Config::default();
    serial_config.dma = DmaConfig::Tx;
    let mut tx = usart3.tx(tx_pin, serial_config, clocks).unwrap();
    writeln!(tx, "\n\n ☢️📶📼 v0.1.0\n\n").unwrap();

    let init_buffer = cortex_m::singleton!(: [u8; LOG_BUFFER_SIZE] = [0; LOG_BUFFER_SIZE]).unwrap();
    let transfer: SerialDma = Transfer::init_memory_to_peripheral(
        StreamsTuple::new(dma1).3,
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
                let mut transfer_idle = TRANSFER_IDLE.borrow(cs).borrow_mut();
                if !*transfer_idle {
                    *transfer_idle = log_queue.is_empty();
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
