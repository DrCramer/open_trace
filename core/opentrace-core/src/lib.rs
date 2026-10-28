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
pub mod peak_morphology;
pub mod small_svd;
pub mod true_peak;
pub mod spike;
pub mod allele_calling;
pub mod kit_definition;
pub mod spike_adjustment;
pub mod quality_control;
pub mod pullup;
pub mod pullup_quality;
pub mod post_sizing_quality;
pub mod ladder_aggregation;
pub mod ladder_observation_binding;
pub mod ladder_calling;
pub mod ladder_quality;
pub mod ladder_quality_workflow;
pub mod marker_quality;
pub mod amel_cross_check;
pub mod marker_quality_workflow;
pub mod control_workflow;
pub mod sample_info;
pub mod profiles;
pub mod pystats;
pub mod run_offsets;
pub mod ladder;
pub mod str_workflow;
pub mod run_workflow;
pub mod gm_kit_import;
pub mod matrix;
pub mod mixture;
pub mod codis;
pub mod export;
pub mod standard_wrapper;
