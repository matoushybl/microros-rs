use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    usb::{Driver, InterruptHandler},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{self, Channel},
};
use embassy_time::{Duration, Instant};
use embassy_usb::class::cdc_acm::{self, CdcAcmClass, State};
use microros_sys::{rmw_uros_set_custom_transport, uxrCustomTransport};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<embassy_rp::peripherals::USB>;
});

pub type MyUsbDriver = Driver<'static, embassy_rp::peripherals::USB>;
pub type MyUsbDevice = embassy_usb::UsbDevice<'static, MyUsbDriver>;

pub fn init_rmw_transport() {
    // TODO: we could check that this runs in thread mode
    unsafe {
        rmw_uros_set_custom_transport(
            true,
            core::ptr::null_mut(),
            Some(transport_open),
            Some(transport_close),
            Some(transport_write),
            Some(transport_read),
        )
    };
}

pub fn usb_config() -> embassy_usb::Config<'static> {
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Moonforge");
    config.product = Some("Eir - robot doctor");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;
    config
}

fn usb_builder(
    usb: embassy_rp::peripherals::USB,
) -> embassy_usb::Builder<'static, embassy_rp::usb::Driver<'static, embassy_rp::peripherals::USB>> {
    let config = usb_config();
    let driver = Driver::new(usb, Irqs);
    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let builder = embassy_usb::Builder::new(
        driver,
        config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [], // no msos descriptors
        CONTROL_BUF.init([0; 64]),
    );
    builder
}

pub fn init_usb(
    peri: embassy_rp::peripherals::USB,
) -> (cdc_acm::CdcAcmClass<'static, MyUsbDriver>, MyUsbDevice) {
    let mut builder = usb_builder(peri);
    let class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());

        CdcAcmClass::new(&mut builder, state, 64)
    };

    let usb = builder.build();

    (class, usb)
}

pub async fn init_usb_transport(peri: embassy_rp::peripherals::USB, spawner: &Spawner) {
    // TODO: we could check that this runs in interrupt mode
    let (mut class, usb) = init_usb(peri);

    defmt::unwrap!(spawner.spawn(usb_task(usb)));

    defmt::error!("waiting for usb");
    class.wait_connection().await;
    defmt::error!("we have usb");

    let (sender, receiver) = class.split();

    defmt::unwrap!(spawner.spawn(sender_task(SENDER_CHANNEL.receiver(), sender)));
    defmt::unwrap!(spawner.spawn(receiver_task(RECEIVER_CHANNEL.sender(), receiver)));
}

#[embassy_executor::task]
pub async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}

pub const BUFFER_LEN: usize = 1024;
pub const QUEUE_LEN: usize = 2;

#[derive(defmt::Format)]
pub struct Buffer {
    inner: [u8; BUFFER_LEN],
    used: usize,
}

impl Buffer {}

pub static SENDER_CHANNEL: Channel<CriticalSectionRawMutex, Buffer, QUEUE_LEN> = Channel::new();
pub static RECEIVER_CHANNEL: Channel<CriticalSectionRawMutex, u8, BUFFER_LEN> = Channel::new();

#[embassy_executor::task]
pub async fn sender_task(
    receiver: channel::Receiver<'static, CriticalSectionRawMutex, Buffer, QUEUE_LEN>,
    mut sender: cdc_acm::Sender<'static, MyUsbDriver>,
) {
    loop {
        let buffer = receiver.receive().await;
        defmt::unwrap!(sender.write_packet(&buffer.inner[..buffer.used]).await);
    }
}

#[embassy_executor::task]
pub async fn receiver_task(
    sender: channel::Sender<'static, CriticalSectionRawMutex, u8, BUFFER_LEN>,
    mut receiver: cdc_acm::Receiver<'static, MyUsbDriver>,
) {
    loop {
        let mut buffer = [0u8; BUFFER_LEN];
        let received = defmt::unwrap!(receiver.read_packet(&mut buffer[..]).await);
        for &b in &buffer[..received] {
            sender.send(b).await;
        }
    }
}
#[no_mangle]
pub extern "C" fn transport_open(_transport: *mut uxrCustomTransport) -> bool {
    true
}

#[no_mangle]
pub extern "C" fn transport_close(_transport: *mut uxrCustomTransport) -> bool {
    true
}

#[no_mangle]
pub extern "C" fn transport_write(
    _transport: *mut uxrCustomTransport,
    buf: *const u8,
    len: usize,
    err: *mut u8,
) -> usize {
    defmt::trace!("write requested: {} bytes", len);
    if len > BUFFER_LEN {
        defmt::panic!("input buffer too large, split into multiple");
    }
    let mut buffer = Buffer {
        inner: [0u8; BUFFER_LEN],
        used: len,
    };
    unsafe {
        buffer.inner[..len].copy_from_slice(core::slice::from_raw_parts(buf, len));
    }
    defmt::unwrap!(SENDER_CHANNEL.try_send(buffer));
    // TODO: we must wait until the data is sent before leaving this function

    len
}

#[no_mangle]
pub extern "C" fn transport_read(
    _transport: *mut uxrCustomTransport,
    buf: *mut u8,
    len: usize,
    timeout: i32,
    err: *mut u8,
) -> usize {
    defmt::trace!("read requested: {}", len);
    let timeout = Duration::from_millis(timeout as u64);
    let start_time = Instant::now();

    let buffer = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    'outer: for i in 0..len {
        while Instant::now() < (start_time + timeout) {
            if let Ok(byte) = RECEIVER_CHANNEL.try_receive() {
                buffer[i] = byte;
                continue 'outer;
            }
        }
        defmt::trace!("timeout while reading");
        unsafe { *err = 1 };
        return i;
    }

    len
}
