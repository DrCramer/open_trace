//! OpenTrace Desktop, native Qt 6 Widgets interface. Research software, not validated for diagnostic or forensic use.
mod backend;
mod bridge;

// Qt support libraries of cxx-qt are linked (their C++ initialisers run before the application starts).
extern crate cxx_qt;
extern crate cxx_qt_lib;

fn main() {
    cxx_qt::init_crate!(opentrace_qt);
    std::process::exit(bridge::ffi::run_app());
}
