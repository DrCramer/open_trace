//! Single entry point for the transcendental functions the analysis uses.
//!
//! `exp`, `log`, `pow` (default build): `libm_port`, a bit-exact Rust port of glibc's FMA variant of these functions —
//! the variant that produced the golden corpora. Same bits on Linux, Windows, with or without FMA hardware (verified
//! against glibc on 10^8 random arguments per function; `tests/libm_port.rs`).
//! `expm1`: also `libm_port` (glibc's fdlibm-based expm1, FMA build; 10^8 arguments identical to glibc). Windows'
//! UCRT expm1 differs by 1–2 ulp in ~11 % of the analysis' arguments, hence the port.
//! `sin`, `frexp`, `ldexp`: the platform C library. `frexp`/`ldexp` are exact. `sin` differs on Windows by 1 ulp in
//! ~2 % of arguments, but the analysis only calls it in the negative-argument reflection of `numeric::lgamma`, which
//! the sizing code never reaches (positive shapes), and in demo-fixture generation (results rounded to integers;
//! the demo golden corpus matches on Windows). glibc's sin is LGPL-licensed IBM code and is deliberately not ported
//! into this MIT crate.
//! Features: `system-libm` uses the C library for everything (old behaviour, for comparisons); `portable-math` uses the
//! pure-Rust `libm` crate (musl port; differs from the reference in 6 golden corpora — experimental).

#[cfg(not(feature = "portable-math"))]
#[allow(dead_code)] // exp/log/pow/expm1 are only used with `system-libm`
mod sys {
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

#[cfg(all(not(feature = "portable-math"), feature = "system-libm"))]
mod imp {
    pub use super::sys::*;
}

#[cfg(all(not(feature = "portable-math"), not(feature = "system-libm")))]
mod imp {
    pub use super::sys::{frexp, ldexp, sin};
    #[inline]
    pub fn exp(x: f64) -> f64 {
        crate::libm_port::exp(x)
    }
    #[inline]
    pub fn expm1(x: f64) -> f64 {
        crate::libm_port::expm1(x)
    }
    #[inline]
    pub fn log(x: f64) -> f64 {
        crate::libm_port::log(x)
    }
    #[inline]
    pub fn pow(x: f64, y: f64) -> f64 {
        crate::libm_port::pow(x, y)
    }
    pub const BACKEND: &str = "glibc-fma-port";
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
