fn main() {
    std::process::exit(opentrace_core::cli::main_with(std::env::args().skip(1).collect()));
}
