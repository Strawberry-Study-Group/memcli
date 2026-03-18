//! Glibc 2.38 compatibility shims.
//!
//! The statically-linked ONNX Runtime (from ort-sys prebuilt binaries)
//! references ISO C23 variants of strto* functions that only exist in
//! glibc 2.38+. On older systems (e.g. glibc 2.35 / Ubuntu 22.04) we
//! provide forwarding wrappers to the standard versions.
//!
//! Also provides `__libc_single_threaded` (glibc 2.32+) for environments
//! where the linker can't find it in the system libc.

use std::os::raw::{c_char, c_int, c_long, c_ulong};

unsafe extern "C" {
    fn strtol(nptr: *const c_char, endptr: *mut *mut c_char, base: c_int) -> c_long;
    fn strtoll(nptr: *const c_char, endptr: *mut *mut c_char, base: c_int) -> i64;
    fn strtoul(nptr: *const c_char, endptr: *mut *mut c_char, base: c_int) -> c_ulong;
    fn strtoull(nptr: *const c_char, endptr: *mut *mut c_char, base: c_int) -> u64;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtol(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> c_long {
    unsafe { strtol(nptr, endptr, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtoll(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> i64 {
    unsafe { strtoll(nptr, endptr, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtoul(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> c_ulong {
    unsafe { strtoul(nptr, endptr, base) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtoull(
    nptr: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> u64 {
    unsafe { strtoull(nptr, endptr, base) }
}

// __libc_single_threaded is a glibc 2.32+ TLS variable that indicates
// whether the process is single-threaded. The ONNX Runtime references it
// for optimizing mutex operations. We provide a static `false` (always
// assume multi-threaded) for environments where the linker can't resolve it.
#[unsafe(no_mangle)]
pub static __libc_single_threaded: u8 = 0; // char, 0 = false (multi-threaded)
