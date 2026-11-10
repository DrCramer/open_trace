#!/usr/bin/env bash
# usage: test-debian10.sh <packages dir> <results dir>   (host with docker; the image is pulled from Docker Hub)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
pkg="$(cd "$1" && pwd)"; out="$2"; mkdir -p "$out"; out="$(cd "$out" && pwd)"
cp "$here/../../tests/check_synthetic.py" "$pkg/check_synthetic.py"
docker run --rm -v "$pkg:/pkg:ro" -v "$out:/out" -v "$here/test-inner.sh:/test-inner.sh:ro" debian:10 bash /test-inner.sh 2>&1 | tee "$out/test-debian10.log"
exit "${PIPESTATUS[0]}"
