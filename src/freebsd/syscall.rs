use core::{
    arch::asm,
    ffi::{c_int, c_void},
};

use super::sysnum::*;
use crate::sc::Arg;

pub type Errno = i64;

macro_rules! syscall {
    ($trap:expr, $arg1:expr) => {
        $crate::freebsd::syscall::syscall3(
            $trap,
            $arg1.into(),
            $crate::sc::Arg::none(),
            $crate::sc::Arg::none(),
        )
    };
    ($trap:expr, $arg1:expr, $arg2:expr) => {
        $crate::freebsd::syscall::syscall3(
            $trap,
            $arg1.into(),
            $arg2.into(),
            $crate::sc::Arg::none(),
        )
    };
    ($trap:expr, $arg1:expr, $arg2:expr, $arg3:expr) => {
        $crate::freebsd::syscall::syscall3($trap, $arg1.into(), $arg2.into(), $arg3.into())
    };
}
pub(crate) use syscall;

pub unsafe fn syscall3(trap: i64, a1: Arg, a2: Arg, a3: Arg) -> Result<(i64, i64), Errno> {
    let r0;
    let r1;
    let err: i64;
    asm!(
        ".cfi_startproc",
        "syscall",
        "setc r8b",
        "movzx {err}, r8b",
        ".cfi_endproc",

        inlateout("rax") trap => r0,
        in("rdi") a1.into_asm(),
        in("rsi") a2.into_asm(),
        inlateout("rdx") a3.into_asm() => r1,
        err = out(reg) err,

        // FreeBSD clobbers these registers.
        out("rcx") _,
        out("r9") _,
        out("r10") _,
        out("r11") _,

        // We clobber `r8b`.
        out("r8b") _,

        options(nostack),
    );
    if err != 0 {
        Err(r0)
    } else {
        Ok((r0, r1))
    }
}
