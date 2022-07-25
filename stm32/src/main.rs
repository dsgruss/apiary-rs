#![no_std]
#![no_main]

use itertools::izip;
use libm::{log10f, powf};
use panic_semihosting as _;
// use panic_itm as _;
// use panic_halt as _;

use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;
use stm32f4xx_hal::{
    adc::{
        config::{AdcConfig, Clock, Continuous, Dma, SampleTime, Scan, Sequence},
        Adc,
    },
    dma::{config, traits::StreamISR, MemoryToPeripheral, Stream3, StreamsTuple, Transfer},
    gpio::GpioExt,
    pac::{self, interrupt, CorePeripherals, Peripherals, DMA1, USART3},
    prelude::*,
    rcc::RccExt,
    serial::{config::DmaConfig, Config, Tx},
};

use core::fmt::{Debug, Write};
use core::{cell::RefCell, iter::zip};
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

use apiary_core::{dsp::{LinearTrap}, socket_smoltcp::SmoltcpInterface, softclip, Module, Uuid, CHANNELS};

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
    let gpiof = p.GPIOF.split();
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

    // TIM2 CH1 : PA15 Red
    // TIM2 CH2 : PB3 Blue
    // TIM3 CH1 : PB4 Green

    // TIM4 CH4 : PB9 Red
    // TIM8 CH1 : PC6 Blue
    // TIM3 CH2 : PC7 Green

    let (mut output_red, mut output_blue) = p
        .TIM2
        .pwm_hz(
            (gpioa.pa15.into_alternate(), gpiob.pb3.into_alternate()),
            1.kHz(),
            &clocks,
        )
        .split();
    let (mut output_green, mut input_green) = p
        .TIM3
        .pwm_hz(
            (gpiob.pb4.into_alternate(), gpioc.pc7.into_alternate()),
            1.kHz(),
            &clocks,
        )
        .split();
    let mut input_red = p
        .TIM4
        .pwm_hz(gpiob.pb9.into_alternate(), 1.kHz(), &clocks)
        .split();
    let mut input_blue = p
        .TIM8
        .pwm_hz(gpioc.pc6.into_alternate(), 1.kHz(), &clocks)
        .split();
    output_red.set_duty(output_red.get_max_duty());
    output_green.set_duty(output_green.get_max_duty() * 0);
    output_blue.set_duty(output_blue.get_max_duty() * 0);
    output_red.enable();
    output_blue.enable();
    output_green.enable();
    input_red.set_duty(input_red.get_max_duty());
    input_green.set_duty(input_green.get_max_duty() * 0);
    input_blue.set_duty(input_blue.get_max_duty() * 0);
    input_red.enable();
    input_blue.enable();
    input_green.enable();

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

    // Allow some time for the interface to come up before starting the IP stack
    let mut cycle_timer = p.TIM5.counter_us(&clocks);
    cycle_timer.start(2.secs()).unwrap();
    nb::block!(cycle_timer.wait()).unwrap();

    let mut storage = Default::default();
    let mut module: Module<_, _, 1, 1> = Module::new(
        SmoltcpInterface::<_, 1, 1, 3>::new(&mut eth_dma, &mut storage),
        rand_source,
        uuid.clone(),
        220,
        0,
    );

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
        let (changed, sw2, sw4) = ui.poll();
        if changed {
            module.set_input_patch_enabled(0, sw2).unwrap();
            module.set_output_patch_enabled(0, sw4).unwrap();
        }
        curr_stats.ui.toc(cycle_timer.now());

        curr_stats.poll.tic(cycle_timer.now());
        match module.poll(time, |input, output| {
            curr_stats.process.tic(cycle_timer.now());
            for j in 0..CHANNELS {
                filters[j].set_params(params[0], params[1]);
            }
            for (fin, fout) in zip(input[0].data, output[0].data.iter_mut()) {
                for (iin, iout, filter) in izip!(fin.data, fout.data.iter_mut(), filters.iter_mut())
                {
                    *iout = (softclip(filter.process(iin as f32 / i16::MAX as f32))
                        * i16::MAX as f32) as i16;
                }
            }
            curr_stats.process.toc(cycle_timer.now());
        }) {
            Ok((input_color, output_color)) => {
                input_red.set_duty(
                    (input_red.get_max_duty() as u32 * (255 - input_color[0].red as u32) / 256)
                        as u16,
                );
                input_green.set_duty(
                    (input_green.get_max_duty() as u32 * (255 - input_color[0].green as u32) / 256)
                        as u16,
                );
                input_blue.set_duty(
                    (input_blue.get_max_duty() as u32 * (255 - input_color[0].blue as u32) / 256)
                        as u16,
                );
                output_red.set_duty(
                    (output_red.get_max_duty() as u32 * (255 - output_color[0].red as u32) / 256)
                        as u16,
                );
                output_green.set_duty(
                    (output_green.get_max_duty() as u32 * (255 - output_color[0].green as u32)
                        / 256) as u16,
                );
                output_blue.set_duty(
                    (output_blue.get_max_duty() as u32 * (255 - output_color[0].blue as u32) / 256)
                        as u16,
                );
            }
            Err(e) => info!("Data send error: {:?}", e),
        }
        curr_stats.poll.toc(cycle_timer.now());

        curr_stats.adc.tic(cycle_timer.now());
        adc_transfer.start(|adc| adc.start_conversion());
        adc_buffer = adc_transfer.next_transfer(adc_buffer).unwrap().0;
        params[0] = 20.0
            * powf(
                10.0,
                (adc_buffer[0] as f32 / 4096.0) * log10f(8000.0 / 20.0),
            );
        params[1] = powf(adc_buffer[1] as f32 / 4096.0, 2.0) * 10.0;
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
