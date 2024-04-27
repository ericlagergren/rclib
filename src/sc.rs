use core::ffi::{c_int, c_void};

pub type RawPtr = *mut c_void;

#[repr(transparent)]
pub struct Arg(RawPtr);

impl Arg {
    pub const fn none() -> Self {
        Self(0 as usize as RawPtr)
    }

    pub const fn into_asm(self) -> *mut c_void {
        self.0
    }
}

impl From<usize> for Arg {
    fn from(v: usize) -> Self {
        Self(v as RawPtr)
    }
}

impl From<c_int> for Arg {
    fn from(v: c_int) -> Self {
        Self(v as usize as RawPtr)
    }
}

impl<T> From<*mut T> for Arg {
    fn from(v: *mut T) -> Self {
        Self(v.cast())
    }
}

impl<T> From<*const T> for Arg {
    fn from(v: *const T) -> Self {
        Self(v.cast::<c_void>().cast_mut())
    }
}

pub type Errno = i64;
