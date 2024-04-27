use core::ffi::{c_int, c_void};

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_os = "freebsd")] {
        use super::freebsd as sys;
    } else {
        compile_error!("unsupported operating system");
    }
}

pub use sys::Errno;

/// See `exit(3)`.
pub fn exit(status: c_int) {
    sys::exit(status);
}

/// See `write(2)`.
pub fn write(filedes: c_int, buf: &[u8]) -> Result<usize, Errno> {
    sys::write(filedes, buf.as_ptr(), buf.len())
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;

    use tempfile::tempfile;

    use super::*;

    #[test]
    fn test_write_basic() {
        let file = tempfile().unwrap();
        let fd = file.as_raw_fd();
        write(fd, &[42u8]).unwrap();
    }
}
