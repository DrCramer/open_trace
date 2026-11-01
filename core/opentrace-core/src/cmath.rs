//! Single entry point for the transcendental functions the analysis uses.
//!
//! Default build: the platform C library (`libm` of glibc on Linux — the platform the Python reference and the golden
//! corpora were produced on). With the `portable-math` feature the pure-Rust `libm` crate (a port of musl's libm) is
//! used instead, which gives the same bits on every OS/CPU. Windows' UCRT `exp`/`pow` differ from glibc by 1 ULP on some
//! inputs (golden corpora `morphology`, `spike`), see docs/desktop/BASELINE_AUDIT.md. Exact operations (`frexp`,
//! `ldexp`) are routed here too so that all libm traffic is in one place.

#[cfg(not(feature = "portable-math"))]
mod imp {
    extern "C" {
        #[link_name = "exp"]
        fn c_exp(x: f64) -> f64;
        #[link_name = "expm1"]
        fn c_expm1(x: f64) -> f64;
        #[link_name = "log"]
        fn c_log(x: f64) -> f64;
        #[link_name = "pow"]
        fn c_pow(x: f64, y: f64) -> f64;
        #[link_name = "sin"]
        fn c_sin(x: f64) -> f64;
        #[link_name = "frexp"]
        fn c_frexp(x: f64, e: *mut i32) -> f64;
        #[link_name = "ldexp"]
        fn c_ldexp(x: f64, e: i32) -> f64;
    }
    #[inline]
    pub fn exp(x: f64) -> f64 {
        unsafe { c_exp(x) }
    }
    #[inline]
    pub fn expm1(x: f64) -> f64 {
        unsafe { c_expm1(x) }
    }
    #[inline]
    pub fn log(x: f64) -> f64 {
        unsafe { c_log(x) }
    }
    #[inline]
    pub fn pow(x: f64, y: f64) -> f64 {
        unsafe { c_pow(x, y) }
    }
    #[inline]
    pub fn sin(x: f64) -> f64 {
        unsafe { c_sin(x) }
    }
    #[inline]
    pub fn frexp(x: f64) -> (f64, i32) {
        let mut e = 0i32;
        let m = unsafe { c_frexp(x, &mut e) };
        (m, e)
    }
    #[inline]
    pub fn ldexp(x: f64, e: i32) -> f64 {
        unsafe { c_ldexp(x, e) }
    }
    pub const BACKEND: &str = "system-libm";
}

#[cfg(feature = "portable-math")]
mod imp {
    #[inline]
    pub fn exp(x: f64) -> f64 {
        libm::exp(x)
    }
    #[inline]
    pub fn expm1(x: f64) -> f64 {
        libm::expm1(x)
    }
    #[inline]
    pub fn log(x: f64) -> f64 {
        libm::log(x)
    }
    #[inline]
    pub fn pow(x: f64, y: f64) -> f64 {
        libm::pow(x, y)
    }
    #[inline]
    pub fn sin(x: f64) -> f64 {
        libm::sin(x)
    }
    #[inline]
    pub fn frexp(x: f64) -> (f64, i32) {
        libm::frexp(x)
    }
    #[inline]
    pub fn ldexp(x: f64, e: i32) -> f64 {
        libm::ldexp(x, e)
    }
    pub const BACKEND: &str = "portable-libm";
}

pub use imp::*;
