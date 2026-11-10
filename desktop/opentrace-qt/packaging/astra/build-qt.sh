#!/usr/bin/env bash
# Qt 6 (qtbase only) from the official sources, built inside the manylinux_2_28 image (glibc 2.28) for Astra Linux.
# Result: $QT_PREFIX (bin/qmake, lib, plugins, LICENSES). Run inside the container, see .github/workflows/native-qt-astra.yml.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"
if [ -f "$QT_PREFIX/.opentrace-qt-built" ] && grep -qx "$QT_VER" "$QT_PREFIX/.opentrace-qt-built"; then
  echo "Qt $QT_VER already built in $QT_PREFIX"; exit 0
fi
set -x
gcc --version | head -1
dnf -y install \
  libxcb-devel xcb-util-devel xcb-util-image-devel xcb-util-keysyms-devel xcb-util-renderutil-devel xcb-util-wm-devel \
  libxkbcommon-devel libxkbcommon-x11-devel libX11-devel libXext-devel libXrender-devel libXi-devel \
  fontconfig-devel freetype-devel cups-devel perl-core xz curl-minimal ca-certificates
"$PYBIN/pip" install --quiet cmake ninja patchelf

work=/tmp/qtbuild
mkdir -p "$work" && cd "$work"
# xcb-util-cursor (needed by the Qt >= 6.5 xcb plugin): distribution package if present, else built from the freedesktop sources
if ! dnf -y install xcb-util-cursor-devel; then
  curl -fsSL -O https://xcb.freedesktop.org/dist/xcb-util-cursor-0.1.4.tar.xz
  tar xf xcb-util-cursor-0.1.4.tar.xz && (cd xcb-util-cursor-0.1.4 && ./configure --prefix=/usr --libdir=/usr/lib64 && make -j"$(nproc)" && make install)
fi
export PKG_CONFIG_PATH=/usr/lib64/pkgconfig:/usr/share/pkgconfig:/usr/lib/pkgconfig

base="https://download.qt.io/archive/qt/${QT_VER%.*}/${QT_VER}/submodules"
src="qtbase-everywhere-src-${QT_VER}.tar.xz"
curl -fsSL -O "$base/$src"
curl -fsSL -O "$base/md5sums.txt"
grep " $src\$" md5sums.txt | md5sum -c -
tar xf "$src"
mkdir build && cd build
../qtbase-everywhere-src-${QT_VER}/configure -prefix "$QT_PREFIX" -release -opensource -confirm-license \
  -nomake examples -nomake tests -no-opengl -no-dbus -no-glib -no-icu -no-openssl \
  -qt-zlib -qt-libpng -qt-libjpeg -qt-harfbuzz -qt-pcre -qt-freetype -fontconfig \
  -xcb -xkbcommon -bundled-xcb-xinput -cups -no-feature-sql -- -G Ninja
cmake --build . --parallel "$(nproc)"
cmake --install .
mkdir -p "$QT_PREFIX/licenses" && cp -r ../qtbase-everywhere-src-${QT_VER}/LICENSES "$QT_PREFIX/licenses/Qt-LICENSES"
cp ../qtbase-everywhere-src-${QT_VER}/LICENSE.LGPL3 "$QT_PREFIX/licenses/" 2>/dev/null || true
{
  echo "Qt $QT_VER (qtbase), built from $base/$src"
  echo "gcc: $(gcc --version | head -1)"
  echo "configure: -prefix $QT_PREFIX -release -opensource -confirm-license -nomake examples -nomake tests -no-opengl -no-dbus -no-glib -no-icu -no-openssl -qt-zlib -qt-libpng -qt-libjpeg -qt-harfbuzz -qt-pcre -qt-freetype -fontconfig -xcb -xkbcommon -bundled-xcb-xinput -cups -no-feature-sql -- -G Ninja"
} > "$QT_PREFIX/qt-build-info.txt"
echo "$QT_VER" > "$QT_PREFIX/.opentrace-qt-built"
