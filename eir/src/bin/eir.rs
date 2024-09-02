#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::*;
use eir::microros;
use eir::microros::Allocator;
use eir::microros::RclNode;
use eir::microros::RclPublisher;
use eir::microros::RclService;
use eir::microros::RclServiceClient;
use eir::microros::RclSubscription;
use eir::microros::RclcExecutor;
use eir::microros::RclcSupport;
use eir::smartled::Ws2812;
use embassy_executor::InterruptExecutor;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio;
use embassy_rp::interrupt;
use embassy_rp::interrupt::InterruptExt as _;
use embassy_rp::interrupt::Priority;
use embassy_rp::pio::Pio;
use embassy_rp::Peripherals;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel;
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use gpio::{Level, Output};
use microros_sys::rosidl_typesupport_c__get_message_type_support_handle__std_msgs__msg__ColorRGBA;
use microros_sys::rosidl_typesupport_c__get_message_type_support_handle__std_msgs__msg__Int32;
use microros_sys::rosidl_typesupport_c__get_service_type_support_handle__std_srvs__srv__SetBool;
use microros_sys::std_msgs__msg__ColorRGBA;
use microros_sys::std_msgs__msg__ColorRGBA__create;
use microros_sys::std_srvs__srv__SetBool_Request;
use microros_sys::std_srvs__srv__SetBool_Request__create;
use microros_sys::std_srvs__srv__SetBool_Response;
use microros_sys::std_srvs__srv__SetBool_Response__create;
use smart_leds::RGB8;
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<embassy_rp::peripherals::PIO0>;
});

#[embassy_executor::task]
async fn run_embassy(p: Peripherals) {
    defmt::info!("hello");
    let spawner = Spawner::for_current_executor().await;

    eir::transport::init_usb_transport(p.USB, &spawner).await;

    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, Irqs);

    let ws2812 = Ws2812::new(&mut common, sm0, p.DMA_CH0, p.PIN_25);

    unwrap!(spawner.spawn(smartled_task(ws2812, SMARTLED_CHANNEL.receiver())));

    let mut led = Output::new(p.PIN_20, Level::Low);
    loop {
        led.set_high();
        Timer::after_millis(300).await;
        led.set_low();
        Timer::after_millis(300).await;
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

    interrupt::SWI_IRQ_0.set_priority(Priority::P3);
    let embassy_spawner = EXECUTOR_EMBASSY.start(interrupt::SWI_IRQ_0);
    unwrap!(embassy_spawner.spawn(run_embassy(p)));

    Timer::after_secs(1).await;

    eir::transport::init_rmw_transport();

    let mut allocator = Allocator::default();

    microros::wait_for_agent();

    let mut support = RclcSupport::new(&mut allocator);
    let mut node = RclNode::new("pico_node", "", &mut support);
    let publisher = RclPublisher::new(
        &mut node,
        unsafe { rosidl_typesupport_c__get_message_type_support_handle__std_msgs__msg__Int32() },
        "pico_publisher",
    );
    defmt::unwrap!(spawner.spawn(publisher_task(publisher)));

    let mut subscription = RclSubscription::new(
        &mut node,
        unsafe {
            rosidl_typesupport_c__get_message_type_support_handle__std_msgs__msg__ColorRGBA()
        },
        "pico_sub",
    );

    let mut executor = RclcExecutor::new(&mut support, 10, &mut allocator);

    let sub_data = unsafe { std_msgs__msg__ColorRGBA__create() };

    executor.add_subscription(&mut subscription, sub_data as _, Some(sub_callback));

    let mut service = RclService::new(
        &mut node,
        unsafe { rosidl_typesupport_c__get_service_type_support_handle__std_srvs__srv__SetBool() },
        "pico_srv",
    );

    executor.add_service(
        &mut service,
        unsafe { std_srvs__srv__SetBool_Request__create() as _ },
        unsafe { std_srvs__srv__SetBool_Response__create() as _ },
        Some(service_callback),
    );

    let mut service_client = RclServiceClient::new(
        &mut node,
        unsafe { rosidl_typesupport_c__get_service_type_support_handle__std_srvs__srv__SetBool() },
        "hello_srv",
    );

    executor.add_service_client(
        &mut service_client,
        unsafe { std_srvs__srv__SetBool_Response__create() as _ },
        Some(service_client_callback),
    );

    defmt::unwrap!(spawner.spawn(service_client_task(service_client)));

    loop {
        yield_now().await;
        executor.spin();
    }
}

extern "C" fn service_client_callback(resp: *const core::ffi::c_void) {
    defmt::warn!("recv");
    defmt::assert!(!resp.is_null());
    defmt::assert!(resp.is_aligned());
    let resp = resp as *const std_srvs__srv__SetBool_Response;
    let resp: &std_srvs__srv__SetBool_Response = unsafe { &*resp as _ };

    defmt::error!("received response: {}", resp.success);
}

#[embassy_executor::task]
async fn service_client_task(mut client: RclServiceClient) {
    let sqn = make_static!(0i64);
    let mut req = std_srvs__srv__SetBool_Request { data: false };

    loop {
        Timer::after_secs(1).await;
        let r: *const std_srvs__srv__SetBool_Request = &req as _;
        client.send_request(r as _, sqn);
        defmt::warn!("req sent");
        req.data = !req.data;
    }
}

extern "C" fn service_callback(req: *const core::ffi::c_void, resp: *mut core::ffi::c_void) {
    defmt::assert!(!req.is_null());
    defmt::assert!(req.is_aligned());
    let req = req as *const std_srvs__srv__SetBool_Request;
    let req: &std_srvs__srv__SetBool_Request = unsafe { &(*req) };
    defmt::error!("service call: {}", req.data);

    defmt::assert!(!resp.is_null());
    defmt::assert!(resp.is_aligned());
    let resp = resp as *mut std_srvs__srv__SetBool_Response;
    let resp: &mut std_srvs__srv__SetBool_Response = unsafe { &mut (*resp) };
    resp.success = true;
}

extern "C" fn sub_callback(data: *const core::ffi::c_void) {
    let real = data as *const std_msgs__msg__ColorRGBA;
    let r = (unsafe { (*real).r } * 255.0) as u8;
    let g = (unsafe { (*real).g } * 255.0) as u8;
    let b = (unsafe { (*real).b } * 255.0) as u8;
    let _ = SMARTLED_CHANNEL.try_send(RGB8 { r, g, b });
    defmt::error!("received: {}", r);
}

#[embassy_executor::task]
async fn publisher_task(mut publisher: RclPublisher) {
    let mut a = 0;
    loop {
        Timer::after_millis(1000).await;
        publisher.publish(a);
        a += 1;
    }
}
