#!/usr/bin/env bash
# Резервная копия проектов и журнала безопасности в runtime/backups/.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p runtime/backups
out="runtime/backups/opentrace_$(date -u +%Y%m%d_%H%M%S).tar.gz"
tar -czf "$out" -C runtime projects security
echo "$out"
