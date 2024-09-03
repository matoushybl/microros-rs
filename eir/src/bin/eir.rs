#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use core::cell::RefCell;

use defmt::*;
use eir::microros;
use eir::microros::Allocator;
use eir::microros::RclNode;
use eir::microros::RclcExecutor;
use eir::microros::RclcSupport;
use eir::microros::TypedPublisher;
use eir::msg::BatteryState;
use eir::msg::Empty;
use eir::smartled::Ws2812;
use embassy_executor::InterruptExecutor;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_rp::adc::Adc;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio;
use embassy_rp::gpio::AnyPin;
use embassy_rp::gpio::Input;
use embassy_rp::gpio::Pin;
use embassy_rp::interrupt;
use embassy_rp::interrupt::InterruptExt as _;
use embassy_rp::interrupt::Priority;
use embassy_rp::pio::Pio;
use embassy_rp::Peripherals;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::CriticalSectionMutex;
use embassy_sync::channel;
use embassy_sync::channel::Channel;
use embassy_time::Instant;
use embassy_time::Timer;
use gpio::{Level, Output};
use microros_sys::builtin_interfaces__msg__Time;
use smart_leds::RGB8;
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<embassy_rp::peripherals::PIO0>;
    ADC_IRQ_FIFO => embassy_rp::adc::InterruptHandler;
});

struct TimestampedValue<T> {
    value: T,
    timestamp: Instant,
}

impl<T> TimestampedValue<T>
where
    T: Copy,
{
    pub fn new(value: T) -> Self {
        Self {
            value,
            timestamp: Instant::now(),
        }
    }

    pub fn set(&mut self, value: T) {
        self.value = value;
        self.timestamp = Instant::now();
    }

    pub fn get(&self) -> (T, Instant) {
        (self.value, self.timestamp)
    }
}

struct State {
    battery_voltage: TimestampedValue<f32>,
}

type SharedState = CriticalSectionMutex<RefCell<State>>;

#[embassy_executor::task]
async fn run_embassy(p: Peripherals, state: &'static SharedState) {
    defmt::info!("hello");
    let spawner = Spawner::for_current_executor().await;

    eir::transport::init_usb_transport(p.USB, &spawner).await;

    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, Irqs);

    let ws2812 = Ws2812::new(&mut common, sm0, p.DMA_CH0, p.PIN_25);

    unwrap!(spawner.spawn(smartled_task(ws2812, SMARTLED_CHANNEL.receiver())));

    let button = Input::new(p.PIN_18.degrade(), gpio::Pull::Up);
    unwrap!(spawner.spawn(shutdown_button_task(button)));

    let adc = Adc::new(p.ADC, Irqs, embassy_rp::adc::Config::default());
    let battery_voltage_channel = embassy_rp::adc::Channel::new_pin(p.PIN_26, gpio::Pull::None);

    unwrap!(spawner.spawn(battery_voltage_measurement_task(
        adc,
        battery_voltage_channel,
        state
    )));

    let mut led = Output::new(p.PIN_20, Level::Low);
    loop {
        led.set_high();
        Timer::after_millis(300).await;
        led.set_low();
        Timer::after_millis(300).await;
    }
}

#[embassy_executor::task]
async fn shutdown_button_task(mut button: Input<'static, AnyPin>) {
    let sender = SHUTDOWN_CHANNEL.sender();
    loop {
        button.wait_for_falling_edge().await;
        // TODO: better debounce?
        Timer::after_millis(100).await;
        sender.send(()).await;
    }
}

#[embassy_executor::task]
async fn battery_voltage_measurement_task(
    mut adc: Adc<'static, embassy_rp::adc::Async>,
    mut channel: embassy_rp::adc::Channel<'static>,
    state: &'static SharedState,
) {
    loop {
        let value = adc.read(&mut channel).await.unwrap_or(0) as f32;
        let voltage = value / 4096.0 * 3.3;
        state.lock(|c| c.borrow_mut().battery_voltage.set(voltage));
        Timer::after_millis(100).await;
    }
}

static SMARTLED_CHANNEL: Channel<CriticalSectionRawMutex, RGB8, 1> = Channel::new();

#[embassy_executor::task]
async fn smartled_task(
    mut driver: Ws2812<'static, embassy_rp::peripherals::PIO0, 0, 5>,
    receiver: channel::Receiver<'static, CriticalSectionRawMutex, RGB8, 1>,
) {
    let mut data = [RGB8::new(0, 0, 0); 5];
    loop {
        let cmd = receiver.receive().await;
        data[0] = cmd;

        driver.write(&data).await;
    }
}

static EXECUTOR_EMBASSY: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn SWI_IRQ_0() {
    EXECUTOR_EMBASSY.on_interrupt()
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let state = make_static!(CriticalSectionMutex::new(RefCell::new(State {
        battery_voltage: TimestampedValue::new(0.0)
    })));

    interrupt::SWI_IRQ_0.set_priority(Priority::P3);
    let embassy_spawner = EXECUTOR_EMBASSY.start(interrupt::SWI_IRQ_0);
    unwrap!(embassy_spawner.spawn(run_embassy(p, state)));

    Timer::after_secs(1).await;

    eir::transport::init_rmw_transport();

    let mut allocator = Allocator::default();

    microros::wait_for_agent();

    let mut support = RclcSupport::new(&mut allocator);
    let mut node = RclNode::new("hati_eir_node", "hati", &mut support);
    let battery_publisher = TypedPublisher::new(&mut node, "battery");
    defmt::unwrap!(spawner.spawn(battery_publisher_task(battery_publisher, state)));

    let shutdown_publisher = TypedPublisher::<Empty>::new(&mut node, "cmd_shutdown");

    defmt::unwrap!(spawner.spawn(shutdown_publisher_task(shutdown_publisher)));

    let mut executor = RclcExecutor::new(&mut support, 10, &mut allocator);

    loop {
        yield_now().await;
        executor.spin();
    }
}

static SHUTDOWN_CHANNEL: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();

#[embassy_executor::task]
async fn shutdown_publisher_task(mut publisher: TypedPublisher<Empty>) {
    let message = Empty::default();
    let receiver = SHUTDOWN_CHANNEL.receiver();
    loop {
        receiver.receive().await;
        publisher.publish(&message);
    }
}

#[embassy_executor::task]
async fn battery_publisher_task(
    mut publisher: TypedPublisher<BatteryState>,
    state: &'static SharedState,
) {
    let mut message = BatteryState::default();
    loop {
        Timer::after_millis(1000).await;
        publisher.publish(&message);
        let (voltage, timestamp) = state.lock(|c| c.borrow().battery_voltage.get());
        message.voltage = voltage;
        message.header.stamp = timestamp.stamp();
    }
}

trait InstantExt {
    fn stamp(&self) -> builtin_interfaces__msg__Time;
}

impl InstantExt for Instant {
    fn stamp(&self) -> builtin_interfaces__msg__Time {
        let now = self.as_micros();

        builtin_interfaces__msg__Time {
            sec: (now / 1_000_000) as _,
            nanosec: (now % 1_000_000) as _,
        }
    }
}
