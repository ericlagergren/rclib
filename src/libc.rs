use core::ffi::{c_int, c_void};

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_os = "freebsd")] {
        use freebsd as sys;
    } else {
        compile_error!("unsupported operating system");
    }
}

/// See `exit(3)`.
#[no_mangle]
pub extern "C" fn exit(status: c_int) {
    sys::exit(status);
}

/// See `write(2)`.
#[no_mangle]
pub extern "C" fn write(filedes: c_int, buf: *const c_void, nbyte: usize) -> isize {
    match sys::write(filedes, buf, nbyte) {
        Ok(n) => n as isize,
        Err(err) => -1,
    }
}
