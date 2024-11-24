#![allow(unused)]
#![no_std]
#![no_main]

use core::fmt::{self, Display};

use display_interface_i2c::I2CInterface;
use embassy_stm32::{mode::Blocking, timer::qei::Qei};
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_10X20, FONT_6X10},
        MonoTextStyleBuilder,
    },
    pixelcolor::BinaryColor,
    prelude::Point,
    text::{Baseline, Text},
};

pub mod pac {
    pub use embassy_stm32::pac::Interrupt as interrupt;
    pub use embassy_stm32::pac::*;
}

type MyDisplay = Ssd1306<
    I2CInterface<embassy_stm32::i2c::I2c<'static, Blocking>>,
    DisplaySize128x32,
    BufferedGraphicsMode<DisplaySize128x32>,
>;

type MyQei = Qei<'static, embassy_stm32::peripherals::TIM2>;

pub struct MyMovAvg {
    acc: MovAvg<f32, f32, 20>,
    last_count: f32,
}

pub mod qei_oversize;

static ENCODER_RATE: f32 = (1024.0 * 4.0);

#[rtic::app(device = crate::pac, peripherals= false, dispatchers = [EXTI0])]
mod app {
    use defmt_rtt as _;
    use display_interface_spi::SPIInterface;
    use embassy_stm32::{
        gpio,
        mode::Blocking,
        spi::{self, Spi},
        time::Hertz,
        timer::{
            low_level::Timer,
            qei::{Qei, QeiPin},
            Channel1Pin,
        },
        Config,
    };
    use embedded_graphics::{
        geometry::Point,
        image::Image,
        mono_font::{
            ascii::{FONT_10X20, FONT_6X10},
            MonoTextStyleBuilder,
        },
        pixelcolor::{BinaryColor, Rgb565},
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle},
        text::{Baseline, Text},
    };
    use movavg::MovAvg;
    use panic_probe as _;

    use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
    use tinybmp::Bmp;

    use crate::{
        draw_numbers, draw_text, qei_oversize::QeiManager, MyDisplay, MyMovAvg, MyQei, ENCODER_RATE,
    };

    #[shared]
    struct SharedResources {}

    #[local]
    struct Resources {
        display: MyDisplay,
        display_timer: Timer<'static, embassy_stm32::peripherals::TIM1>,
        qei_timer: MyQei,
        qei_manager: QeiManager,
        revs_per_minute: MyMovAvg,
        top_left: Point,
        velocity: Point,
        bmp: Bmp<Rgb565, 'static>,
        brightness: Brightness,
    }

    #[init]
    fn init(_cx: init::Context) -> (SharedResources, Resources, init::Monotonics) {
        let mut config: Config = Default::default();
        config.rcc.hse = Some(embassy_stm32::rcc::Hse {
            freq: Hertz::mhz(8),
            mode: embassy_stm32::rcc::HseMode::Oscillator,
        });
        config.rcc.sys = embassy_stm32::rcc::Sysclk::PLL1_P;
        config.rcc.pll = Some(embassy_stm32::rcc::Pll {
            src: embassy_stm32::rcc::PllSource::HSE,
            prediv: embassy_stm32::rcc::PllPreDiv::DIV1,
            mul: embassy_stm32::rcc::PllMul::MUL9, // 8 * 9 = 72Mhz
        });
        // Scale down to 36Mhz (maximum allowed)
        config.rcc.apb1_pre = embassy_stm32::rcc::APBPrescaler::DIV2;

        let p = embassy_stm32::init(config);

        let i2c = embassy_stm32::i2c::I2c::new_blocking(
            p.I2C1,
            p.PB6,
            p.PB7,
            Hertz::khz(400),
            Default::default(),
        );

        let interface = I2CDisplayInterface::new(i2c);
        let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        display.init().unwrap();
        display.set_brightness(Brightness::DIM).unwrap();

        let qei_timer: Qei<'_, embassy_stm32::peripherals::TIM2> =
            Qei::new(p.TIM2, QeiPin::new_ch1(p.PA0), QeiPin::new_ch2(p.PA1));

        let qei_manager = QeiManager::new();

        let revs_per_minute: MyMovAvg = MyMovAvg {
            acc: MovAvg::new(),
            last_count: 0.0,
        };

        // Update framerate
        let display_timer = Timer::new(p.TIM1);
        display_timer.set_frequency(Hertz(20)); // 60 FPS
        display_timer.enable_update_interrupt(true);
        display_timer.start();

        let bmp = Bmp::from_slice(include_bytes!("dvd.bmp")).unwrap();

        // Init the static resources to use them later through RTIC
        (
            SharedResources {},
            Resources {
                display_timer,
                qei_timer,
                qei_manager,
                revs_per_minute,
                display,
                top_left: Point::new(5, 3),
                velocity: Point::new(1, 1),
                bmp,
                brightness: Brightness::default(),
            },
            init::Monotonics(),
        )
    }

    #[task(binds = TIM1_UP, local = [display, top_left, velocity, display_timer, bmp, brightness, qei_timer, qei_manager, revs_per_minute])]
    fn update(cx: update::Context) {
        let update::LocalResources {
            display,
            top_left,
            velocity,
            display_timer,
            bmp,
            brightness,
            qei_timer,
            qei_manager,
            revs_per_minute,
            ..
        } = cx.local;

        let brr = qei_timer.count();
        qei_manager.sample(brr);
        let rev_count = qei_manager.count() as f32 / ENCODER_RATE;

        let rev_diff = rev_count - revs_per_minute.last_count;
        revs_per_minute.last_count = rev_count;

        // если m[1] оборот за 1/n[20] секунды, то за секунду 1 * 20
        // * seconds in minute;
        let revs = rev_diff * 20. * 60.;
        revs_per_minute.acc.feed(revs);

        // draw_text(display);
        display.clear_buffer();

        draw_numbers(display, rev_count as f32, revs_per_minute.acc.get());
        // Write changes to the display
        display.flush().unwrap();

        // Clears the update flag
        display_timer.clear_update_interrupt();
    }
}

use embedded_graphics::Drawable;
use lexical_core::format;
use movavg::MovAvg;
use ssd1306::{mode::BufferedGraphicsMode, size::DisplaySize128x32, Ssd1306};

pub fn draw_text(display: &mut MyDisplay) {
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    Text::with_baseline("Hello world!", Point::zero(), text_style, Baseline::Top)
        .draw(display)
        .unwrap();

    Text::with_baseline("Hello Rust!", Point::new(0, 16), text_style, Baseline::Top)
        .draw(display)
        .unwrap();
}

pub fn draw_numbers(display: &mut MyDisplay, val: f32, val2: f32) {
    let mut buf: heapless::String<40> = heapless::String::new();

    fmt::write(&mut buf, format_args!("{:>10.3} cn", val));
    // let s: &str = core::str::from_utf8(&buf.as_bytes()).unwrap();

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        // .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    Text::with_baseline(&buf, Point::new(0, 16), text_style, Baseline::Top)
        .draw(display)
        .unwrap();

    let mut buf: heapless::String<40> = heapless::String::new();
    fmt::write(&mut buf, format_args!("{:>10.3} r", val2));

    Text::with_baseline(
        &buf,
        // Point::zero(),
        Point::new(0, -2),
        text_style,
        Baseline::Top,
    )
    .draw(display)
    .unwrap();
}
