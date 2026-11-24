#!/bin/sh
# Install the Rudder agent on a server and connect it to a panel.
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
#   --no-edge          Do not start the rudder-traefik and rudder-cache containers
#   --builder          Enable builds; install Docker/Buildx on Debian or Ubuntu if needed
#   --with-nixpacks    With --builder, install pinned Nixpacks 1.41.0
#
# Every value is passed to programs as its own argument; nothing is evaluated.
set -eu

REPO="${RUDDER_GITHUB_REPO:-vantanminh/rudder}"
BIN_DIR="/usr/local/bin"
AGENT_DATA="/var/lib/rudder-agent"
NODE_DATA="/var/lib/rudder"
UNIT="/etc/systemd/system/rudder-agent.service"
TRAEFIK_IMAGE="${RUDDER_TRAEFIK_IMAGE:-traefik:v3.3}"
CACHE_IMAGE="${RUDDER_CACHE_IMAGE:-ghcr.io/vantanminh/rudder-cache:latest}"

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
	printf 'rudder: %s\n' "$1" >&2
	exit 1
}

info() {
	printf 'rudder: %s\n' "$1"
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
		printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/%s %s stable\n' "$(dpkg --print-architecture)" "$ID" "$VERSION_CODENAME" >/etc/apt/sources.list.d/rudder-docker.list
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
asset="rudder-agent-${arch}-unknown-linux-musl"

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

info "downloading $asset"
if fetch "$server/downloads/$asset" "$tmp/agent" && fetch "$server/downloads/$asset.sha256" "$tmp/agent.sha256"; then
	verify "$tmp/agent" "$tmp/agent.sha256" || die "checksum mismatch for the panel's agent binary"
else
	if [ "$version" = "latest" ]; then
		base="https://github.com/$REPO/releases/latest/download"
	else
		base="https://github.com/$REPO/releases/download/$version"
	fi
	fetch "$base/$asset" "$tmp/agent" || die "could not download $asset"
	fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "could not download SHA256SUMS"
	grep " $asset\$" "$tmp/SHA256SUMS" | grep -v "$asset\." >"$tmp/agent.sha256" || die "$asset is not in SHA256SUMS"
	verify "$tmp/agent" "$tmp/agent.sha256" || die "checksum mismatch for $asset"
fi
install -m 0755 "$tmp/agent" "$BIN_DIR/rudder-agent"
info "installed $("$BIN_DIR/rudder-agent" --version)"

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
		if "$BIN_DIR/rudder-agent" --help | grep -q -- --ca-sha256; then
			"$BIN_DIR/rudder-agent" enroll --server "$server" --ca-sha256 "$ca_sha256" --token "$token" --data-dir "$AGENT_DATA"
		else
			info "this rudder-agent cannot check --ca-sha256; enrolling over https:// without the CA pin"
			"$BIN_DIR/rudder-agent" enroll --server "$server" --token "$token" --data-dir "$AGENT_DATA"
		fi
	else
		"$BIN_DIR/rudder-agent" enroll --server "$server" --token "$token" --data-dir "$AGENT_DATA"
	fi
else
	info "already enrolled; keeping $AGENT_DATA (delete it to enroll again)"
fi

if [ "$edge" -eq 1 ]; then
	info "starting rudder-traefik and rudder-cache"
	docker network inspect rudder-proxy >/dev/null 2>&1 || docker network create rudder-proxy >/dev/null
	install -d -m 0755 "$NODE_DATA/traefik" "$NODE_DATA/traefik/dynamic" "$NODE_DATA/traefik/certs" "$NODE_DATA/cache"
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
	} >"$NODE_DATA/traefik/traefik.yml"
	if [ -n "$email" ]; then
		printf '      email: "%s"\n' "$(printf '%s' "$email" | tr -d '"\\\n')" >>"$NODE_DATA/traefik/traefik.yml"
	fi
	[ -f "$NODE_DATA/cache/rules.json" ] || printf '{"version":0,"sites":[],"policies":[]}\n' >"$NODE_DATA/cache/rules.json"
	if ! docker container inspect rudder-traefik >/dev/null 2>&1; then
		docker pull -q "$TRAEFIK_IMAGE" >/dev/null
		docker run -d --name rudder-traefik --restart unless-stopped \
			--network rudder-proxy -p 80:80 -p 443:443 \
			--oom-score-adj -900 --cpu-shares 2048 --memory-reservation 64m \
			-v "$NODE_DATA/traefik:/etc/rudder/traefik" \
			"$TRAEFIK_IMAGE" --configFile=/etc/rudder/traefik/traefik.yml >/dev/null
	fi
	if ! docker container inspect rudder-cache >/dev/null 2>&1; then
		if docker image inspect "$CACHE_IMAGE" >/dev/null 2>&1 || docker pull "$CACHE_IMAGE" >/dev/null 2>&1; then
			docker run -d --name rudder-cache --restart unless-stopped \
				--network rudder-proxy -p 127.0.0.1:9090:9090 \
				-v "$NODE_DATA/cache:/var/lib/rudder/cache" \
				"$CACHE_IMAGE" --rules /var/lib/rudder/cache/rules.json \
				--listen 0.0.0.0:8080 --admin-listen 0.0.0.0:9090 >/dev/null
		else
			info "could not pull $CACHE_IMAGE; caching stays off on this server"
		fi
	fi
fi

install -d -m 0755 "$(dirname "$UNIT")"
cat >"$UNIT" <<'EOF'
[Unit]
Description=Rudder agent
After=network-online.target docker.service
Wants=network-online.target
Requires=docker.service

[Service]
ExecStart=/usr/local/bin/rudder-agent run --data-dir /var/lib/rudder-agent --node-dir /var/lib/rudder
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
systemctl enable rudder-agent >/dev/null 2>&1
systemctl restart rudder-agent
info "rudder-agent is installed; waiting up to 30 seconds for the panel connection"
attempt=0
connected=0
while [ "$attempt" -le 30 ]; do
	if "$BIN_DIR/rudder-agent" status --data-dir "$AGENT_DATA" >"$tmp/status" 2>"$tmp/status-error"; then
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
	die "service installed but not connected; inspect journalctl -u rudder-agent and the panel URL/agent port"
fi
