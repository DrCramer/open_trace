#!/usr/bin/env bash
# System development/runtime libraries (xcb, xkbcommon, X11, fontconfig ...) of the builder image; shared by every CI step.
set -euo pipefail
dnf -y install \
  libxcb-devel xcb-util-devel xcb-util-image-devel xcb-util-keysyms-devel xcb-util-renderutil-devel xcb-util-wm-devel \
  libxkbcommon-devel libxkbcommon-x11-devel libX11-devel libXext-devel libXrender-devel libXi-devel \
  fontconfig-devel freetype-devel cups-devel perl-core

work=/tmp/depsbuild
mkdir -p "$work" && cd "$work"
# xcb-util-cursor (needed by the Qt >= 6.5 xcb plugin): distribution package if present, else built from the freedesktop sources
if ! dnf -y install xcb-util-cursor-devel; then
  curl -fsSL -O https://xcb.freedesktop.org/dist/xcb-util-cursor-0.1.4.tar.xz
  tar xf xcb-util-cursor-0.1.4.tar.xz && (cd xcb-util-cursor-0.1.4 && ./configure --prefix=/usr --libdir=/usr/lib64 && make -j"$(nproc)" && make install)
fi
