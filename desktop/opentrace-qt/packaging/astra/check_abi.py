#!/usr/bin/env python3
"""ABI check of a finished OpenTrace-Qt folder: every ELF file (executable, libraries, Qt plugins).

Fails (exit 1) when
  * a file needs a GLIBC_x symbol version newer than MAX_GLIBC (default 2.28 = Astra Linux 1.7),
  * a file needs GLIBCXX/CXXABI/GCC versions the shipped libstdc++/libgcc_s does not define,
  * a file needs a shared library that is neither in lib/ nor one of the system libraries of sysdeps.py,
  * a run path points outside the folder ($ORIGIN forms only),
  * ldd of the executable or of any plugin reports "not found" for a library of the folder.
Usage: check_abi.py <OpenTrace-Qt folder> [report file]
"""
import os, re, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sysdeps import is_system_lib

root = os.path.abspath(sys.argv[1])
report_path = sys.argv[2] if len(sys.argv) > 2 else None
max_glibc = tuple(int(x) for x in os.environ.get("MAX_GLIBC", "2.28").split("."))
lines, errors = [], []

def say(s=""):
    lines.append(s); print(s)

def vkey(v):
    return tuple(int(x) for x in v.split("."))

def is_elf(p):
    try:
        with open(p, "rb") as f:
            return f.read(4) == b"\x7fELF"
    except OSError:
        return False

elfs = []
for dp, _, fs in os.walk(root):
    for f in fs:
        p = os.path.join(dp, f)
        if not os.path.islink(p) and is_elf(p):
            elfs.append(p)
elfs.sort()

def out(*cmd):
    return subprocess.run(cmd, text=True, capture_output=True).stdout

# versions defined by the shipped runtime libraries
defined = {"GLIBCXX": set(), "CXXABI": set(), "GCC": set()}
for lib in ("libstdc++.so.6", "libgcc_s.so.1"):
    p = os.path.join(root, "lib", lib)
    if os.path.exists(p):
        for line in out("objdump", "-T", p).splitlines():
            if "*UND*" in line:
                continue
            for k in defined:
                m = re.search(rf"\b{k}_([0-9.]+)\b", line)
                if m:
                    defined[k].add(m.group(1))
shipped_cxx = os.path.exists(os.path.join(root, "lib", "libstdc++.so.6"))

say(f"ABI check of {root}  (max glibc {'.'.join(map(str, max_glibc))}, {len(elfs)} ELF files)")
worst_glibc = (0,)
worst_cxx = {}
needed_sys = set()
bundled = {f for f in os.listdir(os.path.join(root, "lib"))} if os.path.isdir(os.path.join(root, "lib")) else set()
for p in elfs:
    rel = os.path.relpath(p, root)
    und = [l for l in out("objdump", "-T", p).splitlines() if "*UND*" in l]
    for l in und:
        for m in re.finditer(r"\bGLIBC_([0-9]+(?:\.[0-9]+)+)\b", l):
            v = vkey(m.group(1))
            worst_glibc = max(worst_glibc, v)
            if v > max_glibc:
                errors.append(f"{rel}: needs GLIBC_{m.group(1)} (> {'.'.join(map(str, max_glibc))}): {l.split()[-1]}")
        for k in ("GLIBCXX", "CXXABI", "GCC"):
            for m in re.finditer(rf"\b{k}_([0-9.]+)\b", l):
                v = m.group(1)
                worst_cxx[k] = max(worst_cxx.get(k, "0"), v, key=lambda s: vkey(s) if re.fullmatch(r"[0-9.]+", s) else (0,))
                if v not in defined[k] and shipped_cxx:
                    errors.append(f"{rel}: needs {k}_{v} that the shipped libstdc++/libgcc_s does not define")
    dyn = out("readelf", "-d", p)
    for m in re.finditer(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", dyn):
        so = m.group(1)
        if so in bundled:
            continue
        if is_system_lib(so):
            needed_sys.add(so)
        elif not so.startswith("libstdc++") and not so.startswith("libgcc_s"):
            errors.append(f"{rel}: needs {so}, neither shipped in lib/ nor a system library")
    for m in re.finditer(r"\((?:RPATH|RUNPATH)\)\s+Library (?:rpath|runpath): \[([^\]]*)\]", dyn):
        for part in m.group(1).split(":"):
            if part and not part.startswith("$ORIGIN"):
                errors.append(f"{rel}: run path {part} points outside the folder")

say(f"highest GLIBC symbol version required by any file: {'.'.join(map(str, worst_glibc))}")
say("highest C++ runtime versions required: " + ", ".join(f"{k}_{v}" for k, v in sorted(worst_cxx.items())))
say("libstdc++ shipped: " + ("yes (defines up to GLIBCXX_" + max(defined["GLIBCXX"], key=vkey) + ")" if shipped_cxx and defined["GLIBCXX"] else "NO"))
say("system libraries the application links against (the operating system provides them):")
for s in sorted(needed_sys):
    say("  " + s)

# ldd of the executable and the platform plugin with only the folder's own library path
for f in ["opentrace-qt", "plugins/platforms/libqxcb.so", "plugins/platforms/libqoffscreen.so"]:
    p = os.path.join(root, f)
    if not os.path.exists(p):
        errors.append(f"{f} is missing")
        continue
    env = {k: v for k, v in os.environ.items() if k != "LD_LIBRARY_PATH"}
    r = subprocess.run(["ldd", p], text=True, capture_output=True, env=env).stdout
    miss = [l.strip() for l in r.splitlines() if "not found" in l]
    for l in miss:
        so = l.split()[0]
        if not is_system_lib(so):   # a system library may be absent in the build image; the test image checks those
            errors.append(f"{f}: ldd: {l}")
    say(f"ldd {f}: {len(miss)} not found (system libraries only is fine in this image)")
for must in ("resources/web/index.html", "resources/data", "launch.sh", "qt.conf", "licenses/THIRD_PARTY_NOTICES.txt", "licenses/OpenTrace-LICENSE-MIT.txt"):
    if not os.path.exists(os.path.join(root, must)):
        errors.append(f"missing in the folder: {must}")
if errors:
    say(); say("INCOMPATIBLE:")
    for e in errors:
        say("  " + e)
else:
    say("OK: all files are compatible with glibc " + ".".join(map(str, max_glibc)))
if report_path:
    open(report_path, "w").write("\n".join(lines) + "\n")
sys.exit(1 if errors else 0)
