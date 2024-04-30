#![cfg(target_os = "freebsd")]

mod imp;
mod syscall;
mod sysnum;

pub use imp::*;
