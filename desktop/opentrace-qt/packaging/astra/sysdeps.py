"""Libraries every Astra Linux desktop provides: they are linked against, never shipped (glibc must be the system's own)."""
import re

# soname prefixes that are NOT bundled
SYSTEM_LIBS = [
    r"linux-vdso", r"ld-linux", r"libc\.so", r"libm\.so", r"libdl\.so", r"libpthread\.so", r"librt\.so", r"libresolv\.so",
    r"libutil\.so", r"libnsl\.so", r"libcrypt\.so", r"libanl\.so",
    r"libGL", r"libEGL", r"libOpenGL", r"libGLX", r"libGLdispatch",
    r"libX11\.so", r"libX11-xcb\.so", r"libXext\.so", r"libXrender\.so", r"libXi\.so", r"libXau\.so", r"libXdmcp\.so",
    r"libfontconfig\.so", r"libfreetype\.so", r"libexpat\.so", r"libuuid\.so", r"libz\.so", r"libbz2\.so", r"libpng1", r"libbrotli",
    r"libcups\.so", r"libgssapi_krb5", r"libkrb5", r"libk5crypto", r"libcom_err", r"libkeyutils", r"libselinux", r"libpcre2?-8",
    r"libavahi", r"libdbus", r"libssl", r"libcrypto", r"libgnutls", r"libsystemd", r"libcap", r"libresolv",
]
_re = re.compile("^(" + "|".join(SYSTEM_LIBS) + ")")

def is_system_lib(soname):
    return bool(_re.match(soname))
