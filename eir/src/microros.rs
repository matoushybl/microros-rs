use core::{marker::PhantomData, mem::MaybeUninit, ptr};

use microros_sys::{
    rcl_client_t, rcl_context_t, rcl_node_t, rcl_publish, rcl_publisher_t, rcl_send_request,
    rcl_service_t, rcl_subscription_t, rclc_client_callback_t, rclc_client_init_default,
    rclc_executor_add_client, rclc_executor_add_service, rclc_executor_add_subscription,
    rclc_executor_handle_invocation_t_ALWAYS, rclc_executor_init, rclc_executor_spin_some,
    rclc_executor_t, rclc_node_init_default, rclc_publisher_init_default, rclc_service_callback_t,
    rclc_service_init_default, rclc_subscription_callback_t, rclc_subscription_init_default,
    rclc_support_init, rclc_support_t, rcutils_allocator_t, rcutils_get_default_allocator,
    rmw_uros_ping_agent, rosidl_message_type_support_t, rosidl_service_type_support_t, RCL_RET_OK,
};

/// Wait for an agent on the host to be available
/// This blocks the current "thread", so the higher priority transport must be running now
pub fn wait_for_agent() {
    defmt::info!("waiting for agent");
    let ret = unsafe { rmw_uros_ping_agent(1000, 10) } as u32;
    defmt::info!("waiting for agent ended {}", ret);
    if ret != RCL_RET_OK {
        defmt::panic!("agent probing failed");
    }
}

pub struct Allocator {
    inner: rcutils_allocator_t,
}

impl Allocator {
    pub fn as_mut_ptr(&mut self) -> *mut rcutils_allocator_t {
        &mut self.inner as _
    }
}

impl Default for Allocator {
    fn default() -> Self {
        Self {
            inner: unsafe { rcutils_get_default_allocator() },
        }
    }
}

pub struct RclcSupport {
    inner: rclc_support_t,
}

impl RclcSupport {
    pub fn new(allocator: &mut Allocator) -> Self {
        let mut raw: MaybeUninit<rclc_support_t> = MaybeUninit::uninit();
        // TODO: this can fail
        unsafe { rclc_support_init(raw.as_mut_ptr(), 0, ptr::null(), allocator.as_mut_ptr()) };

        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut rclc_support_t {
        &mut self.inner as _
    }
}

pub struct RclNode {
    inner: rcl_node_t,
}

impl RclNode {
    pub fn new(node_name: &str, namespace: &str, support: &mut RclcSupport) -> Self {
        let mut raw: MaybeUninit<rcl_node_t> = MaybeUninit::uninit();
        let mut node_name_buf = [0u8; 100]; // TODO: extract to constants
        let mut namespace_buf = [0u8; 100];
        // Note(safety): these are wild assumption about lifetimes of the buffers, but from a quick
        // glance at the code in rcl, it seems like the function then allocates the strings on a
        // heap
        unsafe {
            rclc_node_init_default(
                raw.as_mut_ptr(),
                util::create_null_terminated_string(node_name, &mut node_name_buf),
                util::create_null_terminated_string(namespace, &mut namespace_buf),
                support.as_mut_ptr(),
            );
        }
        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut rcl_node_t {
        &mut self.inner as _
    }
}

pub struct RclPublisher {
    inner: rcl_publisher_t,
}

impl RclPublisher {
    pub fn new(
        node: &mut RclNode,
        message_type: *const rosidl_message_type_support_t,
        topic_name: &str,
    ) -> Self {
        let mut raw: MaybeUninit<rcl_publisher_t> = MaybeUninit::uninit();

        let mut topic_name_buffer = [0u8; 100];

        unsafe {
            rclc_publisher_init_default(
                raw.as_mut_ptr(),
                node.as_mut_ptr(),
                message_type,
                util::create_null_terminated_string(topic_name, &mut topic_name_buffer),
            );
        }
        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut rcl_publisher_t {
        &mut self.inner as _
    }

    pub fn publish(&mut self, data: *const core::ffi::c_void) {
        unsafe { rcl_publish(self.as_mut_ptr(), data, core::ptr::null_mut()) };
    }
}

pub struct TypedPublisher<T> {
    _phantom: PhantomData<T>,
    inner: RclPublisher,
}

impl<T> TypedPublisher<T>
where
    T: crate::msg::Message,
{
    pub fn new(node: &mut RclNode, topic_name: &str) -> Self {
        Self {
            _phantom: PhantomData,
            inner: RclPublisher::new(node, unsafe { T::rosidl_type_support() }, topic_name),
        }
    }

    pub fn publish(&mut self, msg: &T) {
        self.inner.publish(msg.erased_ptr())
    }
}

pub struct RclcExecutor {
    inner: rclc_executor_t,
}

impl RclcExecutor {
    pub fn new(
        support: &mut RclcSupport,
        number_of_handles: usize,
        allocator: &mut Allocator,
    ) -> Self {
        let mut raw: MaybeUninit<rclc_executor_t> = MaybeUninit::uninit();

        unsafe {
            let support: *mut rclc_support_t = support.as_mut_ptr();
            let context: *mut rcl_context_t = &mut (*support).context;

            rclc_executor_init(
                raw.as_mut_ptr(),
                context,
                number_of_handles,
                allocator.as_mut_ptr(),
            );
        }

        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut rclc_executor_t {
        &mut self.inner as _
    }

    pub fn spin(&mut self) {
        unsafe { rclc_executor_spin_some(self.as_mut_ptr(), 100 * 1000 * 1000) };
    }

    pub fn add_subscription(
        &mut self,
        subscription: &mut RclSubscription,
        message: *mut core::ffi::c_void,
        callback: rclc_subscription_callback_t,
    ) {
        unsafe {
            rclc_executor_add_subscription(
                self.as_mut_ptr(),
                subscription.as_mut_ptr(),
                message,
                callback,
                rclc_executor_handle_invocation_t_ALWAYS,
            )
        };
    }

    pub fn add_service(
        &mut self,
        service: &mut RclService,
        request_msg: *mut core::ffi::c_void,
        response_msg: *mut core::ffi::c_void,
        callback: rclc_service_callback_t,
    ) {
        unsafe {
            rclc_executor_add_service(
                self.as_mut_ptr(),
                service.as_mut_ptr(),
                request_msg,
                response_msg,
                callback,
            );
        }
    }

    pub fn add_service_client(
        &mut self,
        client: &mut RclServiceClient,
        response_msg: *mut core::ffi::c_void,
        callback: rclc_client_callback_t,
    ) {
        unsafe {
            rclc_executor_add_client(
                self.as_mut_ptr(),
                client.as_mut_ptr(),
                response_msg,
                callback,
            );
        }
    }
}

pub struct RclSubscription {
    inner: rcl_subscription_t,
}

impl RclSubscription {
    pub fn new(
        node: &mut RclNode,
        message_type: *const rosidl_message_type_support_t,
        topic_name: &str,
    ) -> Self {
        let mut raw = MaybeUninit::uninit();

        let mut topic_name_buffer = [0u8; 100];

        unsafe {
            rclc_subscription_init_default(
                raw.as_mut_ptr(),
                node.as_mut_ptr(),
                message_type,
                util::create_null_terminated_string(topic_name, &mut topic_name_buffer),
            );
        }

        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut rcl_subscription_t {
        &mut self.inner as _
    }
}

pub struct RclService {
    inner: rcl_service_t,
}

impl RclService {
    pub fn new(
        node: &mut RclNode,
        service_type: *const rosidl_service_type_support_t,
        name: &str,
    ) -> Self {
        let mut raw = MaybeUninit::uninit();
        let mut name_buffer = [0u8; 100];
        unsafe {
            rclc_service_init_default(
                raw.as_mut_ptr(),
                node.as_mut_ptr(),
                service_type,
                util::create_null_terminated_string(name, &mut name_buffer),
            );
        }
        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut rcl_service_t {
        &mut self.inner as _
    }
}

pub struct RclServiceClient {
    inner: rcl_client_t,
}

impl RclServiceClient {
    pub fn new(
        node: &mut RclNode,
        type_support: *const rosidl_service_type_support_t,
        name: &str,
    ) -> Self {
        let mut raw = MaybeUninit::uninit();

        let mut name_buffer = [0u8; 100];

        unsafe {
            rclc_client_init_default(
                raw.as_mut_ptr(),
                node.as_mut_ptr(),
                type_support,
                util::create_null_terminated_string(name, &mut name_buffer),
            );
        }

        Self {
            inner: unsafe { raw.assume_init() },
        }
    }

    fn as_mut_ptr(&mut self) -> *mut rcl_client_t {
        &mut self.inner as _
    }

    // TODO: wild assumptions about seq lifetime
    pub fn send_request(&mut self, message: *const core::ffi::c_void, seq: &mut i64) {
        unsafe {
            rcl_send_request(self.as_mut_ptr(), message, seq as _);
        }
    }
}

mod util {
    use core::ffi::c_char;

    pub fn create_null_terminated_string(src: &str, buffer: &mut [u8]) -> *const c_char {
        if !(buffer.len() > src.len()) {
            defmt::panic!("too small buffer to allocate a nul terminated string")
        }

        for (i, &b) in src.as_bytes().iter().enumerate() {
            buffer[i] = b;
        }
        // null terminate
        buffer[src.len()] = 0;

        buffer.as_ptr() as _
    }
}
