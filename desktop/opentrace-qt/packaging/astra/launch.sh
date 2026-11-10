#!/bin/sh
# OpenTrace Qt launcher: works from any directory (also through a symlink such as /usr/bin/opentrace-qt) and needs no root.
here=$(dirname "$(readlink -f "$0")")
: "${OPENTRACE_RESOURCES:=$here/resources}"
export OPENTRACE_RESOURCES
# the Qt libraries and plugins of this folder only (never the system's Qt)
export QT_PLUGIN_PATH="$here/plugins"
export QT_QPA_PLATFORM_PLUGIN_PATH="$here/plugins/platforms"
if [ "$1" = "--diag" ]; then
  echo "OpenTrace Qt folder: $here"
  echo "resources: $OPENTRACE_RESOURCES ($( [ -f "$OPENTRACE_RESOURCES/web/index.html" ] && echo found || echo MISSING ))"
  echo "DISPLAY=$DISPLAY WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
  echo "--- libraries not found:"
  ldd "$here/opentrace-qt" 2>&1 | grep "not found" || echo "(none)"
  ldd "$here/plugins/platforms/libqxcb.so" 2>&1 | grep "not found" || true
  shift
  export QT_DEBUG_PLUGINS=1
fi
exec "$here/opentrace-qt" "$@"
