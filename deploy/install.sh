#!/bin/sh
# Install, restore, or back up the Rudder panel on one Docker host.
#
#   curl -fsSL https://github.com/vantanminh/rudder/releases/latest/download/install.sh | sudo sh
#
# Options:
#   --version VERSION    Release tag of the images, such as v0.2.0 (default: latest)
#   --domain DOMAIN      Public panel domain; Traefik gets a Let's Encrypt certificate for it
#   --email EMAIL        Let's Encrypt account email
#   --restore FILE       Restore a panel backup into a new installation before the panel starts
#   --backup FILE        Write an encrypted panel backup of a running installation to FILE
#   --force              With --restore: replace an existing panel database and master key
#   --no-docker-install  Fail instead of installing Docker when it is missing
#   --close-ports        Stop publishing ports 80, 443, and 3000 (the panel stays on 127.0.0.1:3000);
#                        for servers reached through a Cloudflare tunnel, which must be running
#   --open-ports         Publish those ports again; this is the default
#
# The restore and backup passphrase is read from RUDDER_RESTORE_PASSPHRASE /
# RUDDER_BACKUP_PASSPHRASE, or prompted for on the terminal. It never appears in argv.
# Every value is passed to programs as its own argument; nothing is evaluated.
set -eu

REPO="${RUDDER_GITHUB_REPO:-vantanminh/rudder}"
UPDATE_PUBKEY="${RUDDER_UPDATE_PUBKEY:-__RUDDER_UPDATE_PUBKEY__}"
REGISTRY="${RUDDER_REGISTRY:-ghcr.io/vantanminh}"
TRAEFIK_IMAGE="${RUDDER_TRAEFIK_IMAGE:-traefik:v3.3}"
POSTGRES_IMAGE="${RUDDER_POSTGRES_IMAGE:-postgres:16-alpine}"
ETC="/etc/rudder"
DATA="/var/lib/rudder"
PANEL="rudder"

version="latest"
domain=""
email=""
restore=""
backup=""
force=0
install_docker=1
ports=""

die() {
	printf 'rudder: %s\n' "$1" >&2
	exit 1
}

info() {
	printf 'rudder: %s\n' "$1"
}

need_value() {
	[ "$2" -ge 2 ] || die "$1 needs a value"
}

while [ $# -gt 0 ]; do
	case "$1" in
	--version)
		need_value "$1" $#
		version="$2"
		shift 2
		;;
	--domain)
		need_value "$1" $#
		domain="$2"
		shift 2
		;;
	--email)
		need_value "$1" $#
		email="$2"
		shift 2
		;;
	--restore)
		need_value "$1" $#
		restore="$2"
		shift 2
		;;
	--backup)
		need_value "$1" $#
		backup="$2"
		shift 2
		;;
	--force)
		force=1
		shift
		;;
	--no-docker-install)
		install_docker=0
		shift
		;;
	--close-ports)
		ports="close"
		shift
		;;
	--open-ports)
		ports="open"
		shift
		;;
	-h | --help)
		sed -n '2,20p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'
		exit 0
		;;
	*)
		die "unknown option $1"
		;;
	esac
done

case "$version" in
latest | v[0-9]*) ;;
*) die "--version must look like v1.2.3" ;;
esac
if [ -n "$domain" ]; then
	printf '%s' "$domain" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9.-]{0,251}[A-Za-z0-9])?$' || die "--domain is not a host name"
fi
if [ -n "$email" ]; then
	printf '%s' "$email" | grep -Eq '^[^"[:space:]\\]+@[^"[:space:]\\]+$' || die "--email is not an address"
fi
[ -z "$restore" ] || [ -z "$backup" ] || die "--restore and --backup cannot be combined"
[ "$(id -u)" -eq 0 ] || die "run as root (sudo)"

# Release images are tagged without the leading "v" (v0.2.0 -> :0.2.0).
tag="${version#v}"
PANEL_IMAGE_OVERRIDE="${RUDDER_PANEL_IMAGE:-}"
CACHE_IMAGE_OVERRIDE="${RUDDER_CACHE_IMAGE:-}"
PANEL_IMAGE="${RUDDER_PANEL_IMAGE:-$REGISTRY/rudder:$tag}"
CACHE_IMAGE="${RUDDER_CACHE_IMAGE:-$REGISTRY/rudder-cache:$tag}"

read_secret() {
	# read_secret CURRENT_VALUE ENV_NAME PROMPT -> prints the value
	value="$1"
	if [ -z "$value" ]; then
		[ -r /dev/tty ] || die "set $2; there is no terminal to prompt on"
		printf '%s: ' "$3" >/dev/tty
		stty -echo </dev/tty 2>/dev/null || true
		IFS= read -r value </dev/tty || value=""
		stty echo </dev/tty 2>/dev/null || true
		printf '\n' >/dev/tty
	fi
	[ -n "$value" ] || die "an empty passphrase is not allowed"
	printf '%s' "$value"
}

ensure_docker() {
	if ! command -v docker >/dev/null 2>&1; then
		[ "$install_docker" -eq 1 ] || die "Docker is required: https://docs.docker.com/engine/install/"
		command -v curl >/dev/null 2>&1 || die "curl is required to install Docker"
		info "installing Docker with get.docker.com"
		curl -fsSL https://get.docker.com -o /tmp/rudder-get-docker.sh
		sh /tmp/rudder-get-docker.sh
		rm -f /tmp/rudder-get-docker.sh
	fi
	docker info >/dev/null 2>&1 || die "Docker is installed but not running"
}

random_hex() {
	od -An -N"$1" -tx1 /dev/urandom | tr -d ' \n'
}

verify_minisign() {
	# verify_minisign MESSAGE_FILE SIGNATURE_FILE; the trusted public key is embedded at release time.
	[ -n "$UPDATE_PUBKEY" ] && [ "$UPDATE_PUBKEY" != "__RUDDER_UPDATE_PUBKEY__" ] || return 1
	command -v python3 >/dev/null 2>&1 || return 1
	printf '%s\n' "$UPDATE_PUBKEY" >"$tmp/release.pub"
	python3 - "$tmp/release.pub" "$1" "$2" <<'RUDDER_MINISIGN_PY'
"""Verify a minisign signature with only the Python standard library.

Usage: minisign-verify.py PUBLIC_KEY_BASE64 MESSAGE_FILE SIGNATURE_FILE
Only modern prehashed minisign signatures (algorithm "ED") are accepted.
"""
import base64
import hashlib
import sys

Q = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = (-121665 * pow(121666, Q - 2, Q)) % Q
I = pow(2, (Q - 1) // 4, Q)


def inv(x):
    return pow(x, Q - 2, Q)


def xrecover(y):
    xx = (y * y - 1) * inv(D * y * y + 1) % Q
    x = pow(xx, (Q + 3) // 8, Q)
    if (x * x - xx) % Q:
        x = x * I % Q
    if x % 2:
        x = Q - x
    return x


BY = 4 * inv(5) % Q
B = (xrecover(BY), BY)


def edwards_add(p, q):
    x1, y1 = p
    x2, y2 = q
    common = D * x1 * x2 * y1 * y2 % Q
    x3 = (x1 * y2 + x2 * y1) * inv(1 + common) % Q
    y3 = (y1 * y2 + x1 * x2) * inv(1 - common) % Q
    return x3, y3


def scalarmult(point, scalar):
    result = (0, 1)
    addend = point
    while scalar:
        if scalar & 1:
            result = edwards_add(result, addend)
        addend = edwards_add(addend, addend)
        scalar >>= 1
    return result


def decode_point(encoded):
    if len(encoded) != 32:
        raise ValueError("bad point length")
    y = int.from_bytes(encoded, "little") & ((1 << 255) - 1)
    sign = encoded[31] >> 7
    x = xrecover(y)
    if x % 2 != sign:
        x = Q - x
    if (-x * x + y * y - 1 - D * x * x * y * y) % Q:
        raise ValueError("point is not on the curve")
    return x, y


def ed25519_verify(signature, message, public_key):
    if len(signature) != 64 or len(public_key) != 32:
        return False
    r_encoded = signature[:32]
    s = int.from_bytes(signature[32:], "little")
    if s >= L:
        return False
    try:
        r = decode_point(r_encoded)
        a = decode_point(public_key)
    except ValueError:
        return False
    digest = hashlib.sha512(r_encoded + public_key + message).digest()
    h = int.from_bytes(digest, "little")
    return scalarmult(B, s) == edwards_add(r, scalarmult(a, h))


def parse_public_key(text):
    rows = [line.strip() for line in text.splitlines() if line.strip()]
    encoded = rows[1] if len(rows) > 1 else rows[0]
    raw = base64.b64decode(encoded, validate=True)
    if len(raw) != 42 or raw[:2] not in (b"Ed", b"ED"):
        raise ValueError("unsupported minisign public key")
    return raw[2:10], raw[10:]


def parse_signature(text):
    rows = text.splitlines()
    if len(rows) < 4 or not rows[2].startswith("trusted comment: "):
        raise ValueError("malformed minisign signature")
    first = base64.b64decode(rows[1].strip(), validate=True)
    if len(first) != 74 or first[:2] != b"ED":
        raise ValueError("only prehashed minisign signatures are supported")
    global_sig = base64.b64decode(rows[3].strip(), validate=True)
    if len(global_sig) != 64:
        raise ValueError("malformed minisign global signature")
    return first[2:10], first[10:], rows[2][17:].encode(), global_sig


def main():
    if len(sys.argv) != 4:
        return 2
    try:
        key_id, key = parse_public_key(open(sys.argv[1], encoding="utf-8").read())
        message = open(sys.argv[2], "rb").read()
        sig_key_id, sig, trusted_comment, global_sig = parse_signature(
            open(sys.argv[3], encoding="utf-8").read()
        )
        if key_id != sig_key_id:
            return 1
        digest = hashlib.blake2b(message).digest()
        if not ed25519_verify(sig, digest, key):
            return 1
        if not ed25519_verify(global_sig, sig + trusted_comment, key):
            return 1
    except (OSError, ValueError, base64.binascii.Error):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
RUDDER_MINISIGN_PY
}

fetch() {
	# fetch URL FILE
	curl -fsSL --proto '=https' --retry 3 -o "$2" "$1"
}

database_url() {
	printf 'postgres://rudder:%s@rudder-postgres:5432/rudder' "$(cat "$ETC/postgres.password")"
}

# --- backup of a running installation ---------------------------------------
if [ -n "$backup" ]; then
	ensure_docker
	docker container inspect "$PANEL" >/dev/null 2>&1 || die "the panel container '$PANEL' is not running here"
	passphrase="$(read_secret "${RUDDER_BACKUP_PASSPHRASE:-}" RUDDER_BACKUP_PASSPHRASE 'Backup passphrase (12+ characters)')"
	umask 077
	RUDDER_BACKUP_PASSPHRASE="$passphrase" docker exec -e RUDDER_BACKUP_PASSPHRASE "$PANEL" \
		rudder-server backup --out - >"$backup" || {
		rm -f "$backup"
		die "the backup failed"
	}
	info "panel backup written to $backup ($(wc -c <"$backup") bytes); keep the passphrase with it"
	exit 0
fi

ensure_docker
command -v curl >/dev/null 2>&1 || die "curl is required to verify the signed release"
command -v python3 >/dev/null 2>&1 || die "python3 is required to verify the signed release"

# Verify the signed release manifest before choosing mutable image references.
# The checksum list, its signature, and the manifest all come from the release;
# the minisign key is embedded in this installer when the release is published.
if [ "$version" = "latest" ]; then
	base="https://github.com/$REPO/releases/latest/download"
else
	base="https://github.com/$REPO/releases/download/$version"
fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fetch "$base/release.json" "$tmp/release.json" || die "could not download release.json"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "could not download SHA256SUMS"
fetch "$base/SHA256SUMS.minisig" "$tmp/SHA256SUMS.minisig" || die "could not download SHA256SUMS.minisig"
verify_minisign "$tmp/SHA256SUMS" "$tmp/SHA256SUMS.minisig" || die "the release checksum signature is missing or invalid"
expected_manifest="$(awk '$2 == "release.json" { print $1 }' "$tmp/SHA256SUMS")"
actual_manifest="$(sha256sum "$tmp/release.json" | cut -d ' ' -f 1)"
[ -n "$expected_manifest" ] && [ "$expected_manifest" = "$actual_manifest" ] || die "release.json does not match the signed checksums"
manifest_images="$(python3 - "$tmp/release.json" "$tag" <<'PY'
import json
import sys
manifest=json.load(open(sys.argv[1]))
if manifest.get("format") != "rudder-release":
    raise SystemExit(1)
if sys.argv[2] != "latest" and manifest.get("version") != sys.argv[2]:
    raise SystemExit(1)
images=manifest.get("images", {})
print(images.get("panel", ""))
print(images.get("cache", ""))
PY
)" || die "release.json is invalid or is for another version"
manifest_panel="$(printf '%s\n' "$manifest_images" | sed -n '1p')"
manifest_cache="$(printf '%s\n' "$manifest_images" | sed -n '2p')"
case "$manifest_panel" in *@sha256:[0-9a-f]*) ;; *) die "release.json does not pin the panel image by digest" ;; esac
case "$manifest_cache" in *@sha256:[0-9a-f]*) ;; *) die "release.json does not pin the cache image by digest" ;; esac
[ -n "$PANEL_IMAGE_OVERRIDE" ] || PANEL_IMAGE="$manifest_panel"
[ -n "$CACHE_IMAGE_OVERRIDE" ] || CACHE_IMAGE="$manifest_cache"

if [ -n "$restore" ]; then
	[ -f "$restore" ] || die "$restore does not exist"
	if docker container inspect "$PANEL" >/dev/null 2>&1; then
		[ "$force" -eq 1 ] || die "a panel is already installed; pass --force to replace its data with the backup"
		info "stopping the running panel"
		docker rm -f "$PANEL" >/dev/null
	fi
fi

info "preparing $ETC and $DATA"
install -d -m 0700 "$ETC"
# Closing the ports is remembered, so upgrading later does not reopen them.
if [ "$ports" = "close" ]; then
	# Only a running tunnel replica makes it safe to stop listening publicly.
	[ -n "$(docker ps -q --filter 'name=^rudder-cloudflared-' --filter status=running)" ] ||
		die "no Cloudflare tunnel replica is running; set up the tunnel under Security -> Cloudflare first"
	: >"$ETC/ports.closed"
elif [ "$ports" = "open" ]; then
	rm -f "$ETC/ports.closed"
fi
if [ -f "$ETC/ports.closed" ]; then
	ports_closed=1
	traefik_ports=""
	panel_ports="-p 127.0.0.1:3000:3000 -p 7443:7443"
else
	ports_closed=0
	traefik_ports="-p 80:80 -p 443:443"
	panel_ports="-p 3000:3000 -p 7443:7443"
fi
install -d -m 0755 "$DATA" "$DATA/traefik" "$DATA/traefik/dynamic" "$DATA/traefik/certs" "$DATA/cache"
chown -R 65532:65532 "$DATA/cache" 2>/dev/null || true
[ -f "$DATA/traefik/acme.json" ] || install -m 0600 /dev/null "$DATA/traefik/acme.json"
[ -f "$DATA/cache/rules.json" ] || printf '{"version":0,"sites":[],"policies":[]}\n' >"$DATA/cache/rules.json"
if [ ! -f "$ETC/postgres.password" ]; then
	umask 077
	random_hex 24 >"$ETC/postgres.password"
	umask 022
fi
if [ -z "$restore" ] && [ ! -f "$ETC/master.key" ]; then
	umask 077
	random_hex 32 >"$ETC/master.key"
	umask 022
	info "generated a new master key in $ETC/master.key; back it up or take panel backups"
fi
chmod 0600 "$ETC/postgres.password"
[ ! -f "$ETC/master.key" ] || chmod 0600 "$ETC/master.key"

for network in rudder-proxy rudder-internal; do
	docker network inspect "$network" >/dev/null 2>&1 || docker network create "$network" >/dev/null
done

info "pulling images"
for image in "$POSTGRES_IMAGE" "$TRAEFIK_IMAGE" "$PANEL_IMAGE"; do
	# ":latest" moves with every release, so a copy pulled earlier is stale and
	# an old panel would refuse a database a newer one already migrated.
	if [ "$image" != "$PANEL_IMAGE" ] || [ "$tag" != "latest" ]; then
		docker image inspect "$image" >/dev/null 2>&1 && continue
	fi
	docker pull -q "$image" >/dev/null || die "could not pull $image"
done

# Docker cannot lower the OOM score of a running container. One created before
# the control plane was protected is replaced; its data lives on a volume or in
# $DATA, so nothing is lost.
unprotected() {
	score="$(docker container inspect -f '{{.HostConfig.OomScoreAdj}}' "$1" 2>/dev/null)" || return 1
	[ "${score:-0}" -ge 0 ]
}
if unprotected rudder-postgres; then
	info "recreating rudder-postgres with a protected OOM score"
	docker rm -f rudder-postgres >/dev/null
fi
if unprotected rudder-traefik; then
	info "recreating rudder-traefik with a protected OOM score"
	docker rm -f rudder-traefik >/dev/null
fi

if ! docker container inspect rudder-postgres >/dev/null 2>&1; then
	info "starting rudder-postgres"
	docker run -d --name rudder-postgres --restart unless-stopped \
		--network rudder-internal \
		--oom-score-adj -900 --cpu-shares 8192 --memory-reservation 256m \
		-e POSTGRES_USER=rudder -e POSTGRES_DB=rudder \
		-e POSTGRES_PASSWORD_FILE=/run/secrets/postgres.password \
		-v "$ETC/postgres.password:/run/secrets/postgres.password:ro" \
		-v rudder-postgres:/var/lib/postgresql/data \
		"$POSTGRES_IMAGE" >/dev/null
fi
tries=0
until docker exec rudder-postgres pg_isready -U rudder -d rudder >/dev/null 2>&1; do
	tries=$((tries + 1))
	[ "$tries" -lt 60 ] || die "rudder-postgres did not become ready"
	sleep 1
done

if [ -n "$restore" ]; then
	passphrase="$(read_secret "${RUDDER_RESTORE_PASSPHRASE:-}" RUDDER_RESTORE_PASSPHRASE 'Backup passphrase')"
	info "restoring $restore"
	set -- --file /restore/backup --master-key-file /etc/rudder/master.key
	[ "$force" -eq 0 ] || set -- "$@" --force
	RUDDER_RESTORE_PASSPHRASE="$passphrase" DATABASE_URL="$(database_url)" docker run --rm \
		--network rudder-internal \
		-e RUDDER_RESTORE_PASSPHRASE -e DATABASE_URL \
		-v "$ETC:/etc/rudder" \
		-v "$(cd "$(dirname "$restore")" && pwd)/$(basename "$restore"):/restore/backup:ro" \
		"$PANEL_IMAGE" restore "$@" || die "the restore failed; nothing was started"
	chmod 0600 "$ETC/master.key"
fi

info "writing the Traefik configuration"
{
	echo "entryPoints:"
	echo "  web:"
	echo "    address: \":80\""
	echo "  websecure:"
	echo "    address: \":443\""
	echo "providers:"
	echo "  file:"
	echo "    directory: /etc/rudder/traefik/dynamic"
	echo "    watch: true"
	echo "accessLog:"
	echo "  format: json"
	echo "log:"
	echo "  format: json"
	echo "certificatesResolvers:"
	echo "  letsencrypt:"
	echo "    acme:"
	echo "      storage: /etc/rudder/traefik/acme.json"
	echo "      httpChallenge:"
	echo "        entryPoint: web"
	[ -z "$email" ] || printf '      email: "%s"\n' "$email"
} >"$DATA/traefik/traefik.yml"
# Published ports cannot change on a running container, so Traefik is replaced
# when it listens differently from what was asked for.
if docker container inspect rudder-traefik >/dev/null 2>&1; then
	listening=0
	if docker container inspect -f '{{json .HostConfig.PortBindings}}' rudder-traefik | grep -q '"443/tcp"'; then
		listening=1
	fi
	if [ "$listening" -eq "$ports_closed" ]; then
		info "recreating rudder-traefik to change its published ports"
		docker rm -f rudder-traefik >/dev/null
	fi
fi
if ! docker container inspect rudder-traefik >/dev/null 2>&1; then
	info "starting rudder-traefik"
	# shellcheck disable=SC2086
	docker run -d --name rudder-traefik --restart unless-stopped \
		--network rudder-proxy $traefik_ports \
		--oom-score-adj -900 --cpu-shares 2048 --memory-reservation 64m \
		-v "$DATA/traefik:/etc/rudder/traefik" \
		"$TRAEFIK_IMAGE" --configFile=/etc/rudder/traefik/traefik.yml >/dev/null
fi
CACHE_MEMORY="${RUDDER_CACHE_MEMORY:-512m}"
if ! docker container inspect rudder-cache >/dev/null 2>&1; then
	if docker image inspect "$CACHE_IMAGE" >/dev/null 2>&1 || docker pull -q "$CACHE_IMAGE" >/dev/null 2>&1; then
		info "starting rudder-cache"
		docker run -d --name rudder-cache --restart unless-stopped \
			--network rudder-proxy \
			--memory "$CACHE_MEMORY" --memory-reservation 64m --oom-score-adj 200 \
			-v "$DATA/cache:/var/lib/rudder/cache" \
			"$CACHE_IMAGE" --rules /var/lib/rudder/cache/rules.json \
			--listen 0.0.0.0:8080 --admin-listen 0.0.0.0:9090 >/dev/null
	else
		info "could not pull $CACHE_IMAGE; HTTP caching stays off until it is available"
	fi
fi
if docker container inspect rudder-cache >/dev/null 2>&1; then
	docker update --memory "$CACHE_MEMORY" --memory-reservation 64m rudder-cache >/dev/null 2>&1 || true
fi

address="$(hostname -I 2>/dev/null | cut -d ' ' -f 1)" || address=""
if [ -z "$address" ]; then
	address="$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p')" || address=""
fi
[ -n "$address" ] || address="127.0.0.1"
if [ -n "$domain" ]; then
	public_url="https://$domain"
	agent_url="https://$domain:7443"
else
	public_url="http://$address:3000"
	agent_url="https://$address:7443"
fi

# The panel, its database and Traefik stay ahead of apps: the kernel kills apps
# first when memory runs out, and they get the CPU they need while apps or builds
# are busy. The panel re-applies the CPU weight on start for older installs.
info "starting the panel ($PANEL_IMAGE)"
docker rm -f "$PANEL" >/dev/null 2>&1 || true
DATABASE_URL="$(database_url)" docker run -d --name "$PANEL" --restart unless-stopped \
	--network rudder-internal \
	--oom-score-adj -900 --cpu-shares 8192 --memory-reservation 128m \
	$panel_ports \
	-e DATABASE_URL \
	-e RUDDER_MASTER_KEY_FILE=/etc/rudder/master.key \
	-e RUDDER_DATA_DIR="$DATA" \
	-e RUDDER_HOST_PROC=/host/proc \
	-e RUDDER_CACHE_ADMIN_URL=http://rudder-cache:9090 \
	-e RUDDER_PUBLIC_URL="$public_url" \
	-e RUDDER_AGENT_URL="$agent_url" \
	-e RUDDER_COOKIE_SECURE="$([ -n "$domain" ] && echo true || echo false)" \
	-v /var/run/docker.sock:/var/run/docker.sock \
	-v "$DATA:$DATA" \
	-v "$ETC/master.key:/etc/rudder/master.key:ro" \
	-v /proc:/host/proc:ro \
	"$PANEL_IMAGE" >/dev/null
docker network connect rudder-proxy "$PANEL" >/dev/null 2>&1 || true

tries=0
until docker exec "$PANEL" wget -q -O /dev/null http://127.0.0.1:3000/api/health 2>/dev/null; do
	tries=$((tries + 1))
	if [ "$tries" -ge 60 ]; then
		docker logs --tail 30 "$PANEL" >&2 || true
		die "the panel did not become healthy"
	fi
	sleep 1
done

if [ -n "$restore" ]; then
	info "restored; sign in with the accounts from the backup at $public_url"
elif [ -s "$DATA/setup-token" ]; then
	# The panel writes a one-time token while it has no users; setup refuses requests without it.
	info "Rudder is running. Create the first admin account at $public_url/setup?token=$(tr -d '[:space:]' <"$DATA/setup-token")"
	info "the setup token is also in $DATA/setup-token; keep it private until setup is done"
else
	info "Rudder is running at $public_url; open it to sign in"
fi
[ -z "$domain" ] || info "set the panel domain to $domain under Certificates so Traefik serves it over HTTPS"
[ "$ports_closed" -eq 0 ] || info "ports 80, 443, and 3000 are closed; the panel is reachable only through Cloudflare, or on 127.0.0.1:3000 from this server. Run with --open-ports to undo"
info "back up $ETC/master.key, or run: sh install.sh --backup rudder-panel.tar.zst.enc"
