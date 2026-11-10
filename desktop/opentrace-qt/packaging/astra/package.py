#!/usr/bin/env python3
"""Assemble the self-contained application folder (run inside the manylinux_2_28 container after build-app.sh).

dist/OpenTrace-Qt/
  opentrace-qt  launch.sh  qt.conf
  lib/        Qt 6 libraries and the libraries the Qt platform plugin needs (xcb, xkbcommon, libstdc++ ...)
  plugins/    platforms (xcb, offscreen), imageformats, platforminputcontexts, printsupport
  resources/  web/ data/
  licenses/   OpenTrace, Qt (LGPL-3.0), third-party texts, THIRD_PARTY_NOTICES.txt, build info
The glibc, the dynamic loader and the libraries every Astra Linux desktop has (X11, fontconfig, freetype, GL ...) are NOT
shipped: they are the system's own (see SYSTEM_LIBS in check_abi.py).
"""
import os, re, shutil, subprocess, sys

src = os.environ.get("SRC", "/work")
qt = os.environ.get("QT_PREFIX", "/opt/qt")
out = os.path.join(src, "_astra", "dist", "OpenTrace-Qt")
exe_in = os.path.join(src, "_astra", "target", "release", "opentrace-qt")
here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
from sysdeps import is_system_lib  # noqa: E402

shutil.rmtree(out, ignore_errors=True)
for d in ("lib", "plugins", "licenses"):
    os.makedirs(os.path.join(out, d))
env = dict(os.environ, LD_LIBRARY_PATH=qt + "/lib")

def run(*a, **k):
    return subprocess.run(a, check=True, text=True, **k)

def ldd(path):
    r = subprocess.run(["ldd", path], env=env, text=True, capture_output=True)
    deps = []
    for line in r.stdout.splitlines():
        m = re.match(r"\s*(\S+) => (\S+)", line)
        if m:
            deps.append((m.group(1), m.group(2)))
    return deps

# --- plugins (only what the application uses)
plugin_src = os.path.join(qt, "plugins")
wanted = {
    "platforms": ["libqxcb.so", "libqoffscreen.so", "libqminimal.so"],
    "imageformats": None,  # all
    "platforminputcontexts": None,
    "printsupport": None,
}
for sub, names in wanted.items():
    d = os.path.join(plugin_src, sub)
    if not os.path.isdir(d):
        print("note: no plugin folder", sub)
        continue
    os.makedirs(os.path.join(out, "plugins", sub), exist_ok=True)
    for f in sorted(os.listdir(d)):
        if f.endswith(".so") and (names is None or f in names):
            shutil.copy2(os.path.join(d, f), os.path.join(out, "plugins", sub, f))

shutil.copy2(exe_in, os.path.join(out, "opentrace-qt"))

# --- libraries: transitive closure of the executable and plugins, minus the system's own
roots = [os.path.join(out, "opentrace-qt")]
for dp, _, fs in os.walk(os.path.join(out, "plugins")):
    roots += [os.path.join(dp, f) for f in fs if f.endswith(".so")]
bundled = {}   # soname -> real path
queue = list(roots)
seen = set()
while queue:
    f = queue.pop()
    if f in seen:
        continue
    seen.add(f)
    for soname, path in ldd(f):
        if path == "not":
            raise SystemExit(f"unresolved dependency {soname} of {f}")
        if is_system_lib(soname) or soname in bundled:
            continue
        real = os.path.realpath(path)
        bundled[soname] = real
        queue.append(real)
for soname, real in sorted(bundled.items()):
    dst = os.path.join(out, "lib", soname)
    shutil.copy2(real, dst)
# libstdc++ / libgcc_s of the build compiler when it is newer than the system's: used locally by the application only
gxx = subprocess.run(["gcc", "-print-file-name=libstdc++.so.6"], text=True, capture_output=True).stdout.strip()
if os.path.isabs(gxx) and os.path.exists(gxx):
    shutil.copy2(os.path.realpath(gxx), os.path.join(out, "lib", "libstdc++.so.6"))
    bundled["libstdc++.so.6"] = os.path.realpath(gxx)
gcc_s = subprocess.run(["gcc", "-print-file-name=libgcc_s.so.1"], text=True, capture_output=True).stdout.strip()
if os.path.isabs(gcc_s) and os.path.exists(gcc_s):
    shutil.copy2(os.path.realpath(gcc_s), os.path.join(out, "lib", "libgcc_s.so.1"))
    bundled["libgcc_s.so.1"] = os.path.realpath(gcc_s)

# --- strip and make the folder relocatable: libraries find each other in lib/, plugins reach ../../lib
pe = os.path.join(os.environ.get("PYBIN", "/opt/python/cp312-cp312/bin"), "patchelf")
for dp, _, fs in os.walk(out):
    for f in fs:
        p = os.path.join(dp, f)
        if os.path.islink(p) or not (f.endswith(".so") or ".so." in f or f == "opentrace-qt"):
            continue
        subprocess.run(["strip", "--strip-unneeded", p], check=False, stderr=subprocess.DEVNULL)
        rel = os.path.relpath(os.path.join(out, "lib"), dp)
        origin = "$ORIGIN" + ("" if rel == "." else "/" + rel)
        run(pe, "--set-rpath", origin, p)

# --- resources, launcher, qt.conf, licences
shutil.copytree(os.path.join(src, "resources"), os.path.join(out, "resources"))
for f in ("launch.sh", "qt.conf"):
    shutil.copy2(os.path.join(here, f), os.path.join(out, f))
os.chmod(os.path.join(out, "launch.sh"), 0o755)
lic = os.path.join(out, "licenses")
shutil.copy2(os.path.join(src, "LICENSE"), os.path.join(lic, "OpenTrace-LICENSE-MIT.txt"))
for name in ("Qt-LICENSES", "LICENSE.LGPL3", "qt-build-info.txt"):
    p = os.path.join(qt, "licenses", name) if name != "qt-build-info.txt" else os.path.join(qt, name)
    if os.path.isdir(p):
        shutil.copytree(p, os.path.join(lic, name))
    elif os.path.isfile(p):
        shutil.copy2(p, lic)

# which package every non-Qt library came from, with its licence texts (RPM of the build image)
manifest = []
for soname, real in sorted(bundled.items()):
    if soname.startswith("libQt6") or real.startswith(qt):
        continue
    r = subprocess.run(["rpm", "-qf", "--qf", "%{NAME}-%{VERSION}-%{RELEASE}|%{NAME}|%{LICENSE}", real], text=True, capture_output=True)
    if r.returncode == 0 and "|" in r.stdout:
        nvr, name, lic_name = r.stdout.strip().split("|", 2)
        manifest.append((soname, nvr, lic_name))
        for d in (f"/usr/share/licenses/{name}", f"/usr/share/doc/{name}"):
            if os.path.isdir(d):
                for f in os.listdir(d):
                    if re.match(r"(COPYING|LICEN[CS]E|COPYRIGHT)", f, re.I) and os.path.isfile(os.path.join(d, f)):
                        os.makedirs(os.path.join(lic, name), exist_ok=True)
                        shutil.copy2(os.path.join(d, f), os.path.join(lic, name, f))
    else:
        manifest.append((soname, "(not from an RPM)", "see build image"))

crates = []
try:
    meta = subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", os.path.join(src, "desktop/opentrace-qt/Cargo.toml")],
                          text=True, capture_output=True, check=True).stdout
    import json
    for p in sorted(json.loads(meta)["packages"], key=lambda p: p["name"]):
        if p.get("source"):
            crates.append(f'{p["name"]} {p["version"]}: {p.get("license") or "see crate"}')
except Exception as e:  # noqa: BLE001
    crates.append(f"(crate list not available: {e})")

qtinfo = open(os.path.join(qt, "qt-build-info.txt")).read().strip() if os.path.exists(os.path.join(qt, "qt-build-info.txt")) else ""
notice = f"""OpenTrace Qt (research software, not validated for diagnostic or forensic use) - third-party components

OpenTrace: MIT licence (OpenTrace-LICENSE-MIT.txt). Source: https://github.com/DrCramer/open_trace

Qt 6 (Core, Gui, Widgets, PrintSupport, Network, XcbQpa and plugins) - LGPL-3.0-only (licenses/LICENSE.LGPL3, licenses/Qt-LICENSES).
  Qt is linked dynamically and shipped unmodified in lib/ and plugins/; you may replace these libraries with your own
  build of the same Qt version (the application finds them through $ORIGIN run paths and qt.conf).
  Qt source code: https://download.qt.io/archive/qt/ ; how this Qt was configured and built:
{qtinfo}

Libraries of the build image shipped in lib/ (package, licence):
""" + "\n".join(f"  {s}: {n} - {l}" for s, n, l in manifest) + """

The GNU C library, the dynamic loader and the X11, fontconfig, freetype, OpenGL and CUPS client libraries are NOT shipped:
the application uses those of the operating system.

Rust crates linked into opentrace-qt (name version: licence):
""" + "\n".join("  " + c for c in crates) + "\n"
open(os.path.join(lic, "THIRD_PARTY_NOTICES.txt"), "w").write(notice)
print("dist:", out)
print("bundled libs:", len(bundled))
