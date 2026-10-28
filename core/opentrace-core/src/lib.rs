//! OpenTrace analysis core.
//!
//! Rust ports of the Python reference modules of `genemapper_open` (preprocessing, sg_peaks). Every function
//! reproduces the reference arithmetic exactly (same operation order, `math.fsum` correct rounding, Python
//! floor division and int/int true division), so results are bit-identical; the Python test suite checks it.
pub mod abif;
pub mod analysis;
pub mod sizing_quality;
pub mod autoheight;
pub mod fsum;
pub mod numeric;
pub mod preprocessing;
pub mod prefilters;
pub mod wrapper;
pub mod ratio;
pub mod sg;
pub mod sg_detect;
pub mod sg_weights;
pub mod sizing_pipeline;
pub mod analysis_method;
pub mod calibration;
