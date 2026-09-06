#![no_std]
#![no_main]

use embassy_futures::{block_on, join::join4};
use esp_hal::{
    Async,
    clock::CpuClock,
    rmt::Rmt,
    time::{Instant, Rate},
};
use esp_hal_smartled::{RmtSmartLeds, WS2812B_TIMING, buffer_size, color_order};
use esp_println::println;
use smart_leds_trait::{RGB8, SmartLedsWriteAsync};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const PIXELS: usize = 200;
const PULSES: usize = buffer_size::<RGB8>(PIXELS);
const TRIALS: usize = 10;

type Leds = RmtSmartLeds<'static, PULSES, Async, RGB8, color_order::Rgb>;

static LED0: StaticCell<Leds> = StaticCell::new();
static LED1: StaticCell<Leds> = StaticCell::new();
static LED2: StaticCell<Leds> = StaticCell::new();
static LED3: StaticCell<Leds> = StaticCell::new();
static COLORS: StaticCell<[RGB8; PIXELS]> = StaticCell::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("RMT PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let frequency = Rate::from_mhz(80);
    let rmt = Rmt::new(p.RMT, frequency).unwrap().into_async();
    let led0 = LED0.init_with(|| {
        Leds::new_with_memsize(WS2812B_TIMING, rmt.channel0, p.GPIO13, 2, frequency).unwrap()
    });
    let led1 = LED1.init_with(|| {
        Leds::new_with_memsize(WS2812B_TIMING, rmt.channel2, p.GPIO18, 2, frequency).unwrap()
    });
    let led2 = LED2.init_with(|| {
        Leds::new_with_memsize(WS2812B_TIMING, rmt.channel4, p.GPIO21, 2, frequency).unwrap()
    });
    let led3 = LED3.init_with(|| {
        Leds::new_with_memsize(WS2812B_TIMING, rmt.channel6, p.GPIO25, 2, frequency).unwrap()
    });
    let colors = COLORS.init([RGB8::default(); PIXELS]);
    for (index, color) in colors.iter_mut().enumerate() {
        let value = index as u8;
        *color = RGB8::new(value, value.rotate_left(1), value.rotate_left(3));
    }

    let attach = Instant::now();
    while attach.elapsed().as_millis() < 3000 {}
    println!(
        "RMT PARALLEL BEGIN pixels={} pulses_per_output={} outputs=4 mem_blocks_per_output=2 cpu_mhz={} wifi=off",
        PIXELS,
        PULSES,
        esp_hal::clock::cpu_clock().as_mhz()
    );

    for trial in 0..TRIALS {
        let start = Instant::now();
        let transfer = led0.write(colors.iter().copied());
        let prepare_us = start.elapsed().as_micros();
        block_on(transfer).unwrap();
        println!(
            "RMT SINGLE trial={} prepare_us={} total_us={}",
            trial,
            prepare_us,
            start.elapsed().as_micros()
        );
    }

    for trial in 0..TRIALS {
        let start = Instant::now();
        let transfer0 = led0.write(colors.iter().copied());
        let transfer1 = led1.write(colors.iter().copied());
        let transfer2 = led2.write(colors.iter().copied());
        let transfer3 = led3.write(colors.iter().copied());
        let prepare_us = start.elapsed().as_micros();
        let (result0, result1, result2, result3) =
            block_on(join4(transfer0, transfer1, transfer2, transfer3));
        result0.unwrap();
        result1.unwrap();
        result2.unwrap();
        result3.unwrap();
        println!(
            "RMT FOUR trial={} prepare_us={} total_us={}",
            trial,
            prepare_us,
            start.elapsed().as_micros()
        );
    }
    println!("RMT PARALLEL END");

    loop {
        core::hint::spin_loop();
    }
}
