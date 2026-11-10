# Shared settings of the Astra Linux build (sourced by the other scripts).
# The build runs in the manylinux_2_28 image (AlmaLinux 8, glibc 2.28 = Astra Linux 1.7 / Debian 10 level).
export QT_VER="${QT_VER:-6.8.3}"
export QT_PREFIX="${QT_PREFIX:-/opt/qt}"
export CACHE_DIR="${CACHE_DIR:-/cache}"
export CARGO_HOME="${CARGO_HOME:-$CACHE_DIR/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$CACHE_DIR/rustup}"
export PYBIN="${PYBIN:-/opt/python/cp312-cp312/bin}"
export PATH="$CARGO_HOME/bin:$PYBIN:$PATH"
# highest glibc symbol version allowed in any shipped ELF file (Astra Linux 1.7 has glibc 2.28)
export MAX_GLIBC="${MAX_GLIBC:-2.28}"
