#!/bin/bash
# Runs INSIDE a clean Debian 10 (glibc 2.28, like Astra Linux 1.7) container with only the libraries the DEB depends on.
# mounts: /pkg (the packages, read-only), /out (results, writable)
set -uo pipefail
fail=0
note() { echo "=== $*"; }
check() { if "$@"; then echo "PASS: $*"; else echo "FAIL: $*"; fail=1; fi; }
echo 'deb http://archive.debian.org/debian buster main' > /etc/apt/sources.list
echo 'Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/99archive
export DEBIAN_FRONTEND=noninteractive
note "minimal system libraries (what an Astra desktop has)"
apt-get update -qq >/dev/null
apt-get install -y -qq --no-install-recommends libx11-6 libxext6 libxrender1 libfontconfig1 libfreetype6 libcups2 fonts-dejavu-core xvfb xauth python3 file >/dev/null
ldd --version | head -1
note "portable archive extracted to a path with Cyrillic letters, mounted read-only for the run"
mkdir -p "/opt/тест"
tar -xzf /pkg/OpenTrace-Qt-AstraLinux-1.7-x86_64.tar.gz -C "/opt/тест"
app="/opt/тест/OpenTrace-Qt"
chmod -R a-w "$app"
export HOME=/tmp/home; mkdir -p "$HOME"
selftest() {  # $1 = launcher, $2 = result dir
  local o="$2"; mkdir -p "$o"
  OT_QT_AUTOTEST_FILES=synthetic:5 OT_QT_AUTOTEST_OUT="$o" timeout 600 "$1"
  local rc=$?
  cat "$o/result.json" 2>/dev/null | head -c 400; echo
  python3 - "$o/result.json" <<'PY' || return 1
import json,sys
r=json.load(open(sys.argv[1]))
ok=r['samples']==6 and r['analyzed']==6 and r['failed']==0 and r['pdf'] and r['genotype_rows']>0 and r['reopen_identical'] and r['edit_ok']
sys.exit(0 if ok else 1)
PY
  [ $rc -eq 0 ] || return 1
  python3 "$CHECK" "$o"
}
CHECK=/pkg/check_synthetic.py
note "ldd of the portable build (only system libraries may be absent -> none expected here)"
check bash -c "! ldd '$app/opentrace-qt' | grep 'not found'"
check bash -c "! ldd '$app/plugins/platforms/libqxcb.so' | grep 'not found'"
note "diagnostics script"
QT_QPA_PLATFORM=offscreen timeout 8 "$app/launch.sh" --diag 2>&1 | head -30 || true
note "self-test, offscreen, launcher script, Cyrillic output folder, read-only application folder"
mkdir -p /out/portable-offscreen
QT_QPA_PLATFORM=offscreen check selftest "$app/launch.sh" "/out/portable-offscreen/результаты"
note "self-test on a real X server (Xvfb) with the xcb platform plugin"
check xvfb-run -a -s "-screen 0 1600x1000x24" bash -c "QT_QPA_PLATFORM=xcb OT_QT_AUTOTEST_FILES=synthetic:5 OT_QT_AUTOTEST_OUT=/out/portable-xcb timeout 600 '$app/launch.sh' && python3 /pkg/check_synthetic.py /out/portable-xcb && grep -q '\"failed\":0' /out/portable-xcb/result.json"
note "executable started directly (no launcher): resources and Qt plugins found next to it"
QT_QPA_PLATFORM=offscreen check selftest "$app/opentrace-qt" /out/portable-direct
note "DEB install, run from / through the menu command, remove"
dpkg -i /pkg/opentrace-qt-astra-1.7-amd64.deb || { apt-get install -y -f >/dev/null; dpkg -i /pkg/opentrace-qt-astra-1.7-amd64.deb; }
check test -x /usr/bin/opentrace-qt
check test -f /usr/share/applications/opentrace-qt.desktop
check test -f /usr/share/icons/hicolor/256x256/apps/opentrace-qt.png
cd / && QT_QPA_PLATFORM=offscreen check selftest /usr/bin/opentrace-qt /out/deb-offscreen
cd / && check xvfb-run -a bash -c "QT_QPA_PLATFORM=xcb OT_QT_AUTOTEST_FILES=synthetic:5 OT_QT_AUTOTEST_OUT=/out/deb-xcb timeout 600 /usr/bin/opentrace-qt && grep -q '\"failed\":0' /out/deb-xcb/result.json"
check bash -c "dpkg -L opentrace-qt | grep -c '^/opt/opentrace-qt' | grep -qv '^0\$'"
dpkg -r opentrace-qt
check test ! -e /opt/opentrace-qt
check test ! -e /usr/bin/opentrace-qt
check test ! -e /usr/share/applications/opentrace-qt.desktop
check bash -c "ls $HOME/.local/share/OpenTrace* >/dev/null 2>&1"   # user data lives in the home folder, not in the application folder
echo "glibc of the test system: $(ldd --version | head -1)" > /out/test-system.txt
dpkg-query -W libc6 libx11-6 libfontconfig1 libfreetype6 >> /out/test-system.txt
[ $fail -eq 0 ] && echo "ALL PASSED" || echo "FAILED"
exit $fail
