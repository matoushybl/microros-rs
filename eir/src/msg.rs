use core::ops::{Deref, DerefMut};

use microros_sys::rosidl_message_type_support_t;

// TODO: to achieve "safe" api, these methods should not be available to the user
pub trait Message {
    unsafe fn rosidl_type_support() -> *const rosidl_message_type_support_t;
    fn erased_ptr(&self) -> *const core::ffi::c_void;
    fn erased_mut_ptr(&mut self) -> *mut core::ffi::c_void;
}

macro_rules! generate_msg_wrapper {
    ($wrapper:ident, $msg:path, $create_fn: path, $fini_fn: path, $rosidl_fn: path) => {
        pub struct $wrapper {
            inner: *mut $msg,
        }

        impl Default for $wrapper {
            fn default() -> Self {
                Self {
                    inner: unsafe { $create_fn() },
                }
            }
        }

        impl Drop for $wrapper {
            fn drop(&mut self) {
                unsafe { $fini_fn(self.inner) }
            }
        }

        impl Deref for $wrapper {
            type Target = $msg;
            fn deref(&self) -> &Self::Target {
                unsafe { &*self.inner }
            }
        }

        impl DerefMut for $wrapper {
            fn deref_mut(&mut self) -> &mut Self::Target {
                unsafe { &mut *self.inner }
            }
        }

        impl crate::msg::Message for $wrapper {
            unsafe fn rosidl_type_support() -> *const microros_sys::rosidl_message_type_support_t {
                $rosidl_fn()
            }

            fn erased_ptr(&self) -> *const core::ffi::c_void {
                self.inner as _
            }

            fn erased_mut_ptr(&mut self) -> *mut core::ffi::c_void {
                self.inner as _
            }
        }
    };
}

generate_msg_wrapper!(
    Empty,
    microros_sys::std_msgs__msg__Empty,
    microros_sys::std_msgs__msg__Empty__create,
    microros_sys::std_msgs__msg__Empty__fini,
    microros_sys::rosidl_typesupport_c__get_message_type_support_handle__std_msgs__msg__Empty
);

generate_msg_wrapper!(
    BatteryState,
    microros_sys::sensor_msgs__msg__BatteryState,
    microros_sys::sensor_msgs__msg__BatteryState__create,
    microros_sys::sensor_msgs__msg__BatteryState__fini,
    microros_sys::rosidl_typesupport_c__get_message_type_support_handle__sensor_msgs__msg__BatteryState
);
