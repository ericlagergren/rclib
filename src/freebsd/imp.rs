use core::ffi::{c_int, c_void};

pub use super::syscall::Errno;
use super::{
    syscall::syscall,
    sysnum::{SYS_EXIT, SYS_WRITE},
};

pub fn exit(status: c_int) {
    let _ = syscall!(SYS_EXIT, status);
}

pub fn write(filedes: c_int, buf: *const c_void, nbyte: usize) -> Result<usize, Errno> {
    syscall!(SYS_WRITE, filedes, buf, nbyte).map(|(r0, _)| r0 as usize)
}
