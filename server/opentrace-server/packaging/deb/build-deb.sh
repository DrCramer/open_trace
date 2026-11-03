#!/bin/sh
# Builds opentrace-server_<version>_<arch>.deb from a release binary and the repository's resources.
#   server/opentrace-server/packaging/deb/build-deb.sh [OUT_DIR]      (run from anywhere; needs dpkg-deb)
# Layout: /usr/bin/opentrace-server, /usr/share/opentrace-server/resources, /etc/opentrace/server.toml (conffile),
# /lib/systemd/system/opentrace-server.service. The service is NOT started on install: set the first administrator's
# password first (see postinst message / docs/desktop/DEPLOY_LINUX.md). Data in /var/lib/opentrace-server is never
# removed by the package, not even on purge.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
SRV=$(cd "$HERE/../.." && pwd)
REPO=$(cd "$SRV/../.." && pwd)
OUT=${1:-$SRV/target/release}
BIN=$SRV/target/release/opentrace-server
[ -x "$BIN" ] || { echo "build first: cargo build --release --manifest-path server/opentrace-server/Cargo.toml" >&2; exit 1; }
VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$SRV/Cargo.toml" | head -1)
ARCH=$(dpkg --print-architecture)
ROOT=$(mktemp -d)
chmod 0755 "$ROOT"
trap 'rm -rf "$ROOT"' EXIT
install -D -m 0755 "$BIN" "$ROOT/usr/bin/opentrace-server"
mkdir -p "$ROOT/usr/share/opentrace-server"
cp -r "$REPO/resources" "$ROOT/usr/share/opentrace-server/resources"
find "$ROOT/usr/share/opentrace-server" -type d -exec chmod 0755 {} + ; find "$ROOT/usr/share/opentrace-server" -type f -exec chmod 0644 {} +
install -D -m 0644 "$SRV/packaging/opentrace-server.example.toml" "$ROOT/usr/share/doc/opentrace-server/server.example.toml"
# Packaged configuration: the example plus the resources location of this package.
install -d "$ROOT/etc/opentrace"
{ cat "$SRV/packaging/opentrace-server.example.toml"; printf '\n# Installed by the package.\nresources = "/usr/share/opentrace-server/resources"\n'; } > "$ROOT/etc/opentrace/server.toml"
chmod 0640 "$ROOT/etc/opentrace/server.toml"
sed -e 's#/usr/local/bin/opentrace-server#/usr/bin/opentrace-server#' \
    -e 's#OPENTRACE_RESOURCES=/usr/local/share/opentrace/resources#OPENTRACE_RESOURCES=/usr/share/opentrace-server/resources#' \
    "$SRV/packaging/systemd/opentrace-server.service" > "$ROOT/opentrace-server.service"
install -D -m 0644 "$ROOT/opentrace-server.service" "$ROOT/lib/systemd/system/opentrace-server.service"
rm "$ROOT/opentrace-server.service"
install -d "$ROOT/DEBIAN"
cat > "$ROOT/DEBIAN/control" <<CTL
Package: opentrace-server
Version: $VERSION
Architecture: $ARCH
Maintainer: OpenTrace contributors
Section: science
Priority: optional
Depends: adduser, libc6
Installed-Size: $(du -sk "$ROOT/usr" | cut -f1)
Description: OpenTrace Server (research use only)
 Projects with edit locks, content-addressed FSA store, versioned laboratory
 standards with four-eyes approval, analysis provenance, and the OpenTrace web UI.
 Research software, not validated for diagnostic or forensic casework.
CTL
echo /etc/opentrace/server.toml > "$ROOT/DEBIAN/conffiles"
install -m 0755 "$HERE/postinst" "$HERE/prerm" "$HERE/postrm" "$ROOT/DEBIAN/"
mkdir -p "$OUT"
dpkg-deb --root-owner-group --build "$ROOT" "$OUT/opentrace-server_${VERSION}_${ARCH}.deb"
