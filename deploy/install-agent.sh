#!/bin/sh
# Install the Dymini Panel agent on a server and connect it to a panel.
#
#   curl -fsSL https://panel.example.com/install-agent.sh | sudo sh -s -- \
#     --server https://panel.example.com --ca-sha256 <hex> --token join_...
#
# Options:
#   --server URL       Panel URL (required; https://, or http:// only for localhost)
#   --token TOKEN      One-time join token from the panel (required)
#   --ca-sha256 HEX    SHA-256 of the panel's agent CA; enrollment fails if it differs
#   --version VERSION  Release to fetch from GitHub when the panel has no bundled binary
#   --email EMAIL      Let's Encrypt account email for this server's Traefik
#   --no-edge          Do not start the dymini-panel-traefik and dymini-panel-cache containers
#   --builder          Enable builds; install Docker/Buildx on Debian or Ubuntu if needed
#   --with-nixpacks    With --builder, install pinned Nixpacks 1.41.0
#
# Every value is passed to programs as its own argument; nothing is evaluated.
set -eu

REPO="${DYMINI_PANEL_GITHUB_REPO:-dymini/panel}"
UPDATE_PUBKEY="${DYMINI_PANEL_UPDATE_PUBKEY:-__DYMINI_PANEL_UPDATE_PUBKEY__}"
BIN_DIR="/usr/local/bin"
AGENT_DATA="/var/lib/dymini-panel-agent"
NODE_DATA="/var/lib/dymini-panel"
UNIT="/etc/systemd/system/dymini-panel-agent.service"
TRAEFIK_IMAGE="${DYMINI_PANEL_TRAEFIK_IMAGE:-traefik:v3.3}"
CACHE_IMAGE="${DYMINI_PANEL_CACHE_IMAGE:-ghcr.io/dymini/panel-cache:latest}"

server=""
token=""
ca_sha256=""
version="latest"
email=""
edge=1
builder=0
with_nixpacks=0
NIXPACKS_VERSION="1.41.0"

die() {
	printf 'dymini-panel: %s\n' "$1" >&2
	exit 1
}

info() {
	printf 'dymini-panel: %s\n' "$1"
}

while [ $# -gt 0 ]; do
	case "$1" in
	--server)
		[ $# -ge 2 ] || die "--server needs a value"
		server="$2"
		shift 2
		;;
	--token)
		[ $# -ge 2 ] || die "--token needs a value"
		token="$2"
		shift 2
		;;
	--ca-sha256)
		[ $# -ge 2 ] || die "--ca-sha256 needs a value"
		ca_sha256="$2"
		shift 2
		;;
	--version)
		[ $# -ge 2 ] || die "--version needs a value"
		version="$2"
		shift 2
		;;
	--email)
		[ $# -ge 2 ] || die "--email needs a value"
		email="$2"
		shift 2
		;;
	--no-edge)
		edge=0
		shift
		;;
	--builder)
		builder=1
		edge=0
		shift
		;;
	--with-nixpacks)
		with_nixpacks=1
		shift
		;;
	*)
		die "unknown option $1"
		;;
	esac
done

[ -n "$server" ] || die "--server is required"
[ -n "$token" ] || die "--token is required"
server="${server%/}"
# Enrollment receives the CA the agent trusts from then on, so it must not travel in plain text.
case "$server" in
*@*) die "--server must not contain credentials" ;;
https://*) ;;
http://localhost | http://localhost[:/]* | http://127.0.0.1 | http://127.0.0.1[:/]* | http://\[::1\] | http://\[::1\][:/]*) ;;
http://*) die "--server must use https:// (http:// is allowed only for localhost)" ;;
*) die "--server must start with https://" ;;
esac
case "$ca_sha256" in
"") ;;
*[!0-9a-fA-F]*) die "--ca-sha256 must be 64 hexadecimal characters" ;;
*) [ "${#ca_sha256}" -eq 64 ] || die "--ca-sha256 must be 64 hexadecimal characters" ;;
esac
case "$version" in
latest | v[0-9]*) ;;
*) die "--version must look like v1.2.3" ;;
esac
[ "$(id -u)" -eq 0 ] || die "run as root (sudo)"
command -v curl >/dev/null 2>&1 || die "curl is required"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"
command -v systemctl >/dev/null 2>&1 || die "systemd is required"
[ "$with_nixpacks" -eq 0 ] || [ "$builder" -eq 1 ] || die "--with-nixpacks requires --builder"

docker_apt_repo() {
	[ -f /etc/os-release ] || die "install Docker and its Buildx plugin manually: https://docs.docker.com/engine/install/"
	# shellcheck disable=SC1091
	. /etc/os-release
	case "${ID:-}" in
	debian | ubuntu) ;;
	*) die "automatic Docker/Buildx installation supports Debian and Ubuntu only; install them manually: https://docs.docker.com/engine/install/" ;;
	esac
	case "${VERSION_CODENAME:-}" in
	"" | *[!a-zA-Z0-9._-]*) die "could not determine the Debian/Ubuntu release codename; configure Docker's official apt repository manually" ;;
	esac
	command -v apt-get >/dev/null 2>&1 || die "apt-get is required for automatic Docker installation"
	apt-get update
	apt-get install -y ca-certificates curl
	if ! grep -qs "download.docker.com/linux/$ID" /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; then
		install -d -m 0755 /etc/apt/keyrings
		curl -fsSL --proto '=https' "https://download.docker.com/linux/$ID/gpg" -o /etc/apt/keyrings/docker.asc
		chmod 0644 /etc/apt/keyrings/docker.asc
		printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/%s %s stable\n' "$(dpkg --print-architecture)" "$ID" "$VERSION_CODENAME" >/etc/apt/sources.list.d/dymini-panel-docker.list
	fi
	apt-get update
}

if ! command -v docker >/dev/null 2>&1; then
	[ "$builder" -eq 1 ] || die "Docker is required: https://docs.docker.com/engine/install/"
	docker_apt_repo
	apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin || die "Docker installation failed; resolve package conflicts using https://docs.docker.com/engine/install/"
	systemctl enable --now docker
fi
docker info >/dev/null 2>&1 || die "Docker is installed but not running; start it with systemctl start docker"
if [ "$builder" -eq 1 ] && ! docker buildx version >/dev/null 2>&1; then
	docker_apt_repo
	apt-get install -y docker-buildx-plugin || die "install the Docker Buildx plugin: https://docs.docker.com/build/buildx/install/"
	docker buildx version >/dev/null 2>&1 || die "Docker Buildx is still unavailable; check the Docker CLI plugin installation"
fi

case "$(uname -m)" in
x86_64 | amd64) arch="x86_64" ;;
aarch64 | arm64) arch="aarch64" ;;
*) die "unsupported architecture $(uname -m)" ;;
esac
asset="dymini-panel-agent-${arch}-unknown-linux-musl"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ "$with_nixpacks" -eq 1 ]; then
	command -v bash >/dev/null 2>&1 || die "bash is required by the official Nixpacks installer"
	curl -fsSL --proto '=https' https://nixpacks.com/install.sh -o "$tmp/nixpacks-install.sh"
	NIXPACKS_VERSION="$NIXPACKS_VERSION" NIXPACKS_BIN_DIR="$BIN_DIR" bash "$tmp/nixpacks-install.sh" --force
	[ "$("$BIN_DIR/nixpacks" --version)" = "nixpacks $NIXPACKS_VERSION" ] || die "the installed Nixpacks version does not match pinned $NIXPACKS_VERSION"
fi

fetch() {
	# fetch URL FILE
	curl -fsSL --proto '=https,http' --retry 3 -o "$2" "$1"
}

verify() {
	# verify BINARY CHECKSUM_FILE
	expected="$(cut -d ' ' -f 1 <"$2")"
	actual="$(sha256sum "$1" | cut -d ' ' -f 1)"
	[ -n "$expected" ] && [ "$expected" = "$actual" ]
}

verify_minisign() {
	# verify_minisign MESSAGE_FILE SIGNATURE_FILE; the trusted public key is embedded at release time.
	case "$UPDATE_PUBKEY" in "" | __DYMINI_PANEL_*__) return 1 ;; esac
	command -v python3 >/dev/null 2>&1 || return 1
	printf '%s\n' "$UPDATE_PUBKEY" >"$tmp/release.pub"
	python3 - "$tmp/release.pub" "$1" "$2" <<'DYMINI_PANEL_MINISIGN_PY'
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
DYMINI_PANEL_MINISIGN_PY
}

info "downloading $asset"
# /mcp is the machine-client prefix: the only route family that survives a
# Cloudflare Access agent bypass. A login page can still answer a download
# request with HTML, so only a response that looks like a checksum file
# (a 64-hex digest) counts as the panel's binary; anything else falls
# through to the plain path and then to GitHub.
downloaded=""
for prefix in "$server/mcp/downloads" "$server/downloads"; do
	if fetch "$prefix/$asset" "$tmp/agent" && fetch "$prefix/$asset.sha256" "$tmp/agent.sha256" &&
		grep -qiE '^[0-9a-f]{64}' "$tmp/agent.sha256"; then
		verify "$tmp/agent" "$tmp/agent.sha256" || die "checksum mismatch for the panel's agent binary"
		downloaded=1
		break
	fi
done
if [ -z "$downloaded" ]; then
	if [ "$version" = "latest" ]; then
		base="https://github.com/$REPO/releases/latest/download"
	else
		base="https://github.com/$REPO/releases/download/$version"
	fi
	fetch "$base/$asset" "$tmp/agent" || die "could not download $asset"
	fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "could not download SHA256SUMS"
	fetch "$base/SHA256SUMS.minisig" "$tmp/SHA256SUMS.minisig" || die "could not download SHA256SUMS.minisig"
	verify_minisign "$tmp/SHA256SUMS" "$tmp/SHA256SUMS.minisig" || die "the release checksum signature is missing or invalid; install python3 or set DYMINI_PANEL_UPDATE_PUBKEY"
	grep " $asset\$" "$tmp/SHA256SUMS" | grep -v "$asset\." >"$tmp/agent.sha256" || die "$asset is not in SHA256SUMS"
	verify "$tmp/agent" "$tmp/agent.sha256" || die "checksum mismatch for $asset"
fi
install -m 0755 "$tmp/agent" "$BIN_DIR/dymini-panel-agent"
info "installed $("$BIN_DIR/dymini-panel-agent" --version)"

# --- migrate a pre-rebrand Rudder agent ---------------------------------------
# The agent state (enrollment, mTLS keys) lived in rudder-named directories and
# a rudder-agent.service unit; carry it over so the host does not re-enroll.
if [ -d /var/lib/rudder-agent ] && [ ! -e "$AGENT_DATA/agent.json" ]; then
	info "migrating the pre-rebrand Rudder agent state"
	install -d -m 0700 "$AGENT_DATA"
	cp -a /var/lib/rudder-agent/. "$AGENT_DATA/"
fi
if [ -d /var/lib/rudder ] && [ ! -e "$NODE_DATA/traefik" ]; then
	install -d -m 0755 "$NODE_DATA"
	cp -a /var/lib/rudder/. "$NODE_DATA/"
fi
if [ -f /etc/systemd/system/rudder-agent.service ]; then
	systemctl disable --now rudder-agent.service >/dev/null 2>&1 || true
	rm -f /etc/systemd/system/rudder-agent.service
	systemctl daemon-reload >/dev/null 2>&1 || true
fi
# The old edge containers hold ports 80/443, so they must go before the
# rebranded ones start; deployed services keep running untouched.
for old_edge in rudder-traefik rudder-cache; do
	docker rm -f "$old_edge" >/dev/null 2>&1 && info "removed the old $old_edge container" || true
done


install -d -m 0700 "$AGENT_DATA"
if [ "$builder" -eq 1 ]; then
	install -m 0600 /dev/null "$AGENT_DATA/config.json.tmp"
	printf '{"builder":true}\n' >"$AGENT_DATA/config.json.tmp"
	mv "$AGENT_DATA/config.json.tmp" "$AGENT_DATA/config.json"
	install -d -m 0700 "$AGENT_DATA/builder"
fi
install -d -m 0755 "$NODE_DATA"
if [ ! -f "$AGENT_DATA/agent.json" ]; then
	if [ -n "$ca_sha256" ]; then
		if "$BIN_DIR/dymini-panel-agent" --help | grep -q -- --ca-sha256; then
			"$BIN_DIR/dymini-panel-agent" enroll --server "$server" --ca-sha256 "$ca_sha256" --token "$token" --data-dir "$AGENT_DATA"
		else
			info "this dymini-panel-agent cannot check --ca-sha256; enrolling over https:// without the CA pin"
			"$BIN_DIR/dymini-panel-agent" enroll --server "$server" --token "$token" --data-dir "$AGENT_DATA"
		fi
	else
		"$BIN_DIR/dymini-panel-agent" enroll --server "$server" --token "$token" --data-dir "$AGENT_DATA"
	fi
else
	info "already enrolled; keeping $AGENT_DATA (delete it to enroll again)"
fi

if [ "$edge" -eq 1 ]; then
	info "starting dymini-panel-traefik and dymini-panel-cache"
	docker network inspect dymini-panel-proxy >/dev/null 2>&1 || docker network create dymini-panel-proxy >/dev/null
	install -d -m 0755 "$NODE_DATA/traefik" "$NODE_DATA/traefik/dynamic" "$NODE_DATA/traefik/certs" "$NODE_DATA/cache"
	chown -R 65532:65532 "$NODE_DATA/cache" 2>/dev/null || true
	if [ ! -f "$NODE_DATA/traefik/acme.json" ]; then
		install -m 0600 /dev/null "$NODE_DATA/traefik/acme.json"
	fi
	{
		echo "entryPoints:"
		echo "  web:"
		echo "    address: \":80\""
		echo "  websecure:"
		echo "    address: \":443\""
		echo "providers:"
		echo "  file:"
		echo "    directory: /etc/dymini-panel/traefik/dynamic"
		echo "    watch: true"
		echo "accessLog:"
		echo "  format: json"
		echo "log:"
		echo "  format: json"
		echo "certificatesResolvers:"
		echo "  letsencrypt:"
		echo "    acme:"
		echo "      storage: /etc/dymini-panel/traefik/acme.json"
		echo "      httpChallenge:"
		echo "        entryPoint: web"
	} >"$NODE_DATA/traefik/traefik.yml"
	if [ -n "$email" ]; then
		printf '      email: "%s"\n' "$(printf '%s' "$email" | tr -d '"\\\n')" >>"$NODE_DATA/traefik/traefik.yml"
	fi
	[ -f "$NODE_DATA/cache/rules.json" ] || printf '{"version":0,"sites":[],"policies":[]}\n' >"$NODE_DATA/cache/rules.json"
	if ! docker container inspect dymini-panel-traefik >/dev/null 2>&1; then
		docker pull -q "$TRAEFIK_IMAGE" >/dev/null
		docker run -d --name dymini-panel-traefik --restart unless-stopped \
			--network dymini-panel-proxy -p 80:80 -p 443:443 \
			--oom-score-adj -900 --cpu-shares 2048 --memory-reservation 64m \
			-v "$NODE_DATA/traefik:/etc/dymini-panel/traefik" \
			"$TRAEFIK_IMAGE" --configFile=/etc/dymini-panel/traefik/traefik.yml >/dev/null
	fi
	# Services deployed before the rebrand still sit on rudder-proxy; Traefik
	# resolves them by container name there until their next deploy.
	if docker network inspect rudder-proxy >/dev/null 2>&1; then
		docker network connect rudder-proxy dymini-panel-traefik >/dev/null 2>&1 || true
	fi
	if ! docker container inspect dymini-panel-cache >/dev/null 2>&1; then
		if docker image inspect "$CACHE_IMAGE" >/dev/null 2>&1 || docker pull "$CACHE_IMAGE" >/dev/null 2>&1; then
			docker run -d --name dymini-panel-cache --restart unless-stopped \
				--network dymini-panel-proxy -p 127.0.0.1:9090:9090 \
				-v "$NODE_DATA/cache:/var/lib/dymini-panel/cache" \
				"$CACHE_IMAGE" --rules /var/lib/dymini-panel/cache/rules.json \
				--listen 0.0.0.0:8080 --admin-listen 0.0.0.0:9090 >/dev/null
		else
			info "could not pull $CACHE_IMAGE; caching stays off on this server"
		fi
	fi
fi

install -d -m 0755 "$(dirname "$UNIT")"
cat >"$UNIT" <<'EOF'
[Unit]
Description=Dymini Panel agent
After=network-online.target docker.service
Wants=network-online.target
Requires=docker.service

[Service]
ExecStart=/usr/local/bin/dymini-panel-agent run --data-dir /var/lib/dymini-panel-agent --node-dir /var/lib/dymini-panel
Restart=always
RestartSec=5
Environment=RUST_LOG=info
# Keep the agent answering when apps use up the server, so the panel can still stop them.
OOMScoreAdjust=-900
CPUWeight=1000
MemoryLow=32M

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable dymini-panel-agent >/dev/null 2>&1
systemctl restart dymini-panel-agent
info "dymini-panel-agent is installed; waiting up to 30 seconds for the panel connection"
attempt=0
connected=0
while [ "$attempt" -le 30 ]; do
	if "$BIN_DIR/dymini-panel-agent" status --data-dir "$AGENT_DATA" >"$tmp/status" 2>"$tmp/status-error"; then
		connected=1
		break
	fi
	attempt=$((attempt + 1))
	[ "$attempt" -le 30 ] || break
	sleep 1
done
info "installation summary (builder storage: $AGENT_DATA/builder)"
cat "$tmp/status"
if [ "$connected" -ne 1 ]; then
	cat "$tmp/status-error" >&2
	die "service installed but not connected; inspect journalctl -u dymini-panel-agent and the panel URL/agent port"
fi
