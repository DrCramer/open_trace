#!/usr/bin/env bash
# DEB and portable tar.gz from the finished OpenTrace-Qt folder (host side; needs dpkg-deb and tar).
# usage: make-packages.sh <dist/OpenTrace-Qt> <output dir> [version]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
dist="$(cd "$1" && pwd)"; out="$2"; mkdir -p "$out"; out="$(cd "$out" && pwd)"
ver="${3:-0.1.0}"
tar_name="OpenTrace-Qt-AstraLinux-1.7-x86_64.tar.gz"
deb_name="opentrace-qt-astra-1.7-amd64.deb"

# portable archive: OpenTrace-Qt/ ... (launch.sh, lib/, plugins/, resources/, qt.conf)
parent="$(dirname "$dist")"
tar --owner=0 --group=0 --numeric-owner --sort=name -C "$parent" -czf "$out/$tar_name" "$(basename "$dist")"

# DEB: /opt/opentrace-qt (application), /usr/bin/opentrace-qt (launcher link), menu entry and icons
root="$(mktemp -d)"; trap 'rm -rf "$root"' EXIT
install -d "$root/opt" "$root/usr/bin" "$root/usr/share/applications" "$root/usr/share/icons/hicolor/256x256/apps" "$root/usr/share/icons/hicolor/scalable/apps" "$root/usr/share/doc/opentrace-qt" "$root/DEBIAN"
cp -a "$dist" "$root/opt/opentrace-qt"
ln -s /opt/opentrace-qt/launch.sh "$root/usr/bin/opentrace-qt"
install -m 644 "$here/opentrace-qt.desktop" "$root/usr/share/applications/opentrace-qt.desktop"
install -m 644 "$here/opentrace-qt.png" "$root/usr/share/icons/hicolor/256x256/apps/opentrace-qt.png"
install -m 644 "$here/opentrace-qt.svg" "$root/usr/share/icons/hicolor/scalable/apps/opentrace-qt.svg"
install -m 644 "$dist/licenses/OpenTrace-LICENSE-MIT.txt" "$root/usr/share/doc/opentrace-qt/copyright"
install -m 644 "$dist/licenses/THIRD_PARTY_NOTICES.txt" "$root/usr/share/doc/opentrace-qt/THIRD_PARTY_NOTICES.txt"
find "$root/opt" -type d -exec chmod 755 {} +
find "$root/opt" -type f -exec chmod 644 {} +
chmod 755 "$root/opt/opentrace-qt/opentrace-qt" "$root/opt/opentrace-qt/launch.sh"
find "$root/opt/opentrace-qt/lib" "$root/opt/opentrace-qt/plugins" -name '*.so*' -type f -exec chmod 755 {} +
size_kb=$(du -sk "$root/opt" "$root/usr" | awk '{s+=$1} END {print s}')
cat > "$root/DEBIAN/control" <<CTL
Package: opentrace-qt
Version: $ver
Section: science
Priority: optional
Architecture: amd64
Installed-Size: $size_kb
Depends: libc6 (>= 2.28), libx11-6, libxext6, libxrender1, libfontconfig1, libfreetype6
Recommends: libcups2, fonts-dejavu-core | fonts-liberation
Maintainer: OpenTrace contributors <noreply@example.invalid>
Homepage: https://github.com/DrCramer/open_trace
Description: OpenTrace - fragment analysis, native Qt 6 interface (research software)
 Open-source reimplementation of fragment analysis (FSA/HID reading, sizing,
 allele calling, quality values, plots). Qt 6 libraries are shipped inside
 the package (/opt/opentrace-qt/lib); the system Qt and glibc are not touched.
 Research software: not validated for diagnostic or forensic use.
CTL
cat > "$root/DEBIAN/postinst" <<'SH'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
fi
exit 0
SH
cat > "$root/DEBIAN/postrm" <<'SH'
#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = purge ]; then
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
fi
exit 0
SH
chmod 755 "$root/DEBIAN/postinst" "$root/DEBIAN/postrm"
# xz: understood by the dpkg of Astra Linux 1.7 (dpkg 1.19)
dpkg-deb --root-owner-group -Zxz -b "$root" "$out/$deb_name"
dpkg-deb -I "$out/$deb_name" | head -20
ls -l "$out"
