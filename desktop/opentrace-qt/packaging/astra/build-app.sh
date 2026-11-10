#!/usr/bin/env bash
# Release build of opentrace-qt against the Qt of $QT_PREFIX (inside the manylinux_2_28 container).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"
src="${SRC:-/work}"
set -x
if ! command -v cargo >/dev/null; then
  curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable --no-modify-path
fi
rustc --version; cargo --version
"$PYBIN/pip" install --quiet patchelf
export QMAKE="$QT_PREFIX/bin/qmake"
export LD_LIBRARY_PATH="$QT_PREFIX/lib"
export CARGO_TARGET_DIR="$src/_astra/target"
cargo build --release --manifest-path "$src/desktop/opentrace-qt/Cargo.toml"
ls -l "$CARGO_TARGET_DIR/release/opentrace-qt"
