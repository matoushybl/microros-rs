#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::*;
use eir::microros;
use eir::microros::Allocator;
use eir::microros::RclNode;
use eir::microros::RclServiceClient;
use eir::microros::RclcExecutor;
use eir::microros::RclcSupport;
use embassy_executor::InterruptExecutor;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_rp::gpio;
use embassy_rp::interrupt;
use embassy_rp::interrupt::InterruptExt as _;
use embassy_rp::interrupt::Priority;
use embassy_rp::Peripherals;
use embassy_time::Timer;
use gpio::{Level, Output};
use microros_sys::rosidl_typesupport_c__get_service_type_support_handle__std_srvs__srv__SetBool;
use microros_sys::std_srvs__srv__SetBool_Request;
use microros_sys::std_srvs__srv__SetBool_Request__create;
use microros_sys::std_srvs__srv__SetBool_Response;
use microros_sys::std_srvs__srv__SetBool_Response__create;
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::task]
async fn run_embassy(p: Peripherals) {
    defmt::info!("hello");
    let spawner = Spawner::for_current_executor().await;

    eir::transport::init_usb_transport(p.USB, &spawner).await;

    let mut led = Output::new(p.PIN_20, Level::Low);
    loop {
        led.set_high();
        Timer::after_millis(300).await;
        led.set_low();
        Timer::after_millis(300).await;
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
    let mut executor = RclcExecutor::new(&mut support, 10, &mut allocator);

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
    defmt::assert!(!resp.is_null());
    defmt::assert!(resp.is_aligned());
    let resp = resp as *const std_srvs__srv__SetBool_Response;
    let resp: &std_srvs__srv__SetBool_Response = unsafe { &*resp as _ };

    defmt::info!("received response: {}", resp.success);
}

#[embassy_executor::task]
async fn service_client_task(mut client: RclServiceClient) {
    let sqn = make_static!(0i64);
    let req = unsafe { std_srvs__srv__SetBool_Request__create() };

    loop {
        Timer::after_secs(1).await;
        client.send_request(req as _, sqn);
        defmt::info!("req sent");
        let req: &mut std_srvs__srv__SetBool_Request = unsafe { &mut *req as _ };
        req.data = !req.data;
    }
}
