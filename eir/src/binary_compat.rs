use embassy_time::Instant;
use microros_sys::timespec;

#[no_mangle]
extern "C" fn _fini() {
    defmt::panic!("Called _fini symbol, which should not be used on microcontrollers. This symbol's origin is in newlib.")
}

#[no_mangle]
extern "C" fn clock_gettime(_clock_id: microros_sys::__clockid_t, tp: *mut timespec) -> i32 {
    let us = Instant::now().as_micros() as i64;
    unsafe {
        (*tp).tv_sec = us / 1_000_000;
        (*tp).tv_nsec = ((us % 1_000_000) * 1000) as i32;
    }
    0
}
