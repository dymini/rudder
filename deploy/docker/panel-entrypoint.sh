#!/bin/sh
# Prepare mounted state as root, then replace this process with the panel as a
# dedicated non-root user. Docker socket access is granted through a numeric
# supplementary group because its GID is host-dependent.
set -eu

PANEL_UID=10001
PANEL_GID=10001
CACHE_GID=65532
DATA_DIR="${DYMINI_PANEL_DATA_DIR:-/var/lib/dymini-panel}"
HOME_DIR=/home/dymini-panel

mkdir -p "$DATA_DIR" "$HOME_DIR"
chown "$PANEL_UID:$PANEL_GID" "$HOME_DIR" 2>/dev/null || true
chmod 0755 "$HOME_DIR" 2>/dev/null || true

# Older installations mount the master key as a root-only read-only file. Copy
# it into the container's private /run while root is still available, then use
# that copy after dropping privileges.
if [ -n "${DYMINI_PANEL_MASTER_KEY_FILE:-}" ] && [ -f "$DYMINI_PANEL_MASTER_KEY_FILE" ]; then
	mkdir -p /run/dymini-panel
	cp "$DYMINI_PANEL_MASTER_KEY_FILE" /run/dymini-panel/master.key
	chown "$PANEL_UID:$PANEL_GID" /run/dymini-panel/master.key
	chmod 0600 /run/dymini-panel/master.key
	export DYMINI_PANEL_MASTER_KEY_FILE=/run/dymini-panel/master.key
fi

chown "$PANEL_UID:$PANEL_GID" "$DATA_DIR" 2>/dev/null || true
chmod 0755 "$DATA_DIR" 2>/dev/null || true

# Everything outside the edge-service directories is control-plane state. Use
# -h so a hostile symlink in the data directory cannot make this migration
# chown a target outside it.
for path in "$DATA_DIR"/*; do
	[ -e "$path" ] || break
	name="${path##*/}"
	if [ "$name" != cache ] && [ "$name" != traefik ]; then
		chown -Rh "$PANEL_UID:$PANEL_GID" "$path" 2>/dev/null || true
	fi
done

# The cache process runs as 65532:65532. Keep object ownership with it while
# sharing the directory and panel-maintained files through its group. The panel
# receives that group as a supplementary group below.
if [ -d "$DATA_DIR/cache" ]; then
	chown 65532:"$CACHE_GID" "$DATA_DIR/cache" 2>/dev/null || true
	chmod 2775 "$DATA_DIR/cache" 2>/dev/null || true
	for file in rules.json admin.token; do
		if [ -f "$DATA_DIR/cache/$file" ]; then
			chown "$PANEL_UID:$CACHE_GID" "$DATA_DIR/cache/$file" 2>/dev/null || true
			chmod 0664 "$DATA_DIR/cache/$file" 2>/dev/null || true
		fi
	done
fi

# Traefik reads this directory; the panel owns and writes it. Traefik's own
# acme.json stays outside it and is not shared with the panel.
if [ -d "$DATA_DIR/traefik" ]; then
	mkdir -p "$DATA_DIR/traefik/dynamic"
	chown -Rh "$PANEL_UID:$PANEL_GID" "$DATA_DIR/traefik/dynamic" 2>/dev/null || true
	chmod 2775 "$DATA_DIR/traefik/dynamic" 2>/dev/null || true
	find "$DATA_DIR/traefik/dynamic" -type f -exec chmod 0660 {} + 2>/dev/null || true
fi

GROUPS="$CACHE_GID"
if [ -S /var/run/docker.sock ]; then
	DOCKER_GID="$(stat -c '%g' /var/run/docker.sock 2>/dev/null || printf '%s' "$PANEL_GID")"
	case ",$GROUPS," in
	*,"$DOCKER_GID",*) ;;
	*) GROUPS="$GROUPS,$DOCKER_GID" ;;
	esac
fi

export HOME="$HOME_DIR"
export USER=dymini-panel
export LOGNAME=dymini-panel
exec setpriv --reuid="$PANEL_UID" --regid="$PANEL_GID" --groups="$GROUPS" \
	/usr/local/bin/dymini-panel-server "$@"
