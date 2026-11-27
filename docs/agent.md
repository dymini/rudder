# Remote servers and the agent

A remote server runs `dymini-panel-agent`, a single static binary. The agent dials the panel, runs the panel's Docker calls on its own machine, and ships that machine's logs and metrics back. The panel never connects to the server, so servers behind NAT or a firewall work as long as they can reach the panel.

```
panel :7443 (gRPC, mTLS)  ◀── dial out ──  dymini-panel-agent ── Docker Engine
                                             └─ dymini-panel-traefik, dymini-panel-cache, apps
```

When the panel's agent port is unreachable — for example the panel sits behind a
Cloudflare Tunnel with every inbound port closed — the agent carries the same
mTLS session inside a WebSocket on the panel's HTTPS address
(`/mcp/api/v1/agent/tunnel`). The fallback is automatic: the agent dials the
agent port first and uses the tunnel when that fails, then remembers the
working transport for reconnects. Cloudflare only ever relays WebSocket bytes;
the end-to-end mTLS handshake is unchanged.

## Add a server

1. In **Servers**, add a server with kind `remote`. The panel shows a one-line command with a one-time join token that expires in an hour.
2. Run the command on the server as root:

   ```bash
   curl -fsSL https://panel.example.com/mcp/install-agent.sh | sudo sh -s -- \
     --server https://panel.example.com --ca-sha256 3f6c... --token join_...
   ```

   The `/mcp` prefix is the machine-client path family; the plain
   `/install-agent.sh` URL keeps working too.

   The panel URL must use `https://`; `http://` is refused unless it points at `localhost`, `127.0.0.1`, or `[::1]`, because enrollment receives the certificate authority the agent trusts from then on. `--ca-sha256` is the SHA-256 fingerprint of the panel's agent CA, and the panel puts it in the command it shows. If the CA the panel returns has a different fingerprint, enrollment stops and nothing is saved.

   The installer needs Docker, systemd, `curl`, and `sha256sum`. It:
   - downloads `dymini-panel-agent` for the machine's architecture from the panel (`/mcp/downloads/...`, then `/downloads/...`), or from the GitHub release when the panel has no bundled copy or a login page answers instead, and checks its SHA-256;
   - runs `dymini-panel-agent enroll`, which creates a private key on the server and exchanges the token and a certificate request for a client certificate;
   - creates the `dymini-panel-proxy` network and starts `dymini-panel-traefik` (ports 80 and 443, JSON access log) and `dymini-panel-cache` (admin port on `127.0.0.1:9090` only);
   - installs and starts the `dymini-panel-agent` systemd unit.
3. The server turns **online** within a few seconds. The Servers page shows the agent version, hostname, OS, Docker version, and last contact.

Options: `--version v1.2.3` pins the GitHub release, `--email` sets the Let's Encrypt account email, and `--no-edge` skips Traefik and the cache. Running the installer again upgrades the binary and keeps the existing identity. To move a server to a new identity, use **Join command** on the server row to get a fresh token, delete `/var/lib/dymini-panel-agent` on the server, and rerun the command. The old certificate stops working as soon as the new one is issued.

## Agent update verification

Before checking a downloaded agent's version, the agent flushes and syncs the staged binary and closes its writable handle. If another starting process briefly inherited that handle, Linux can still report `Text file busy`. The version check retries only that spawn error, at most 10 attempts with 100 ms between attempts (900 ms of backoff), within the existing 15-second check timeout. Other execution errors fail immediately.

## What works on a remote server

Everything the local server supports goes through the agent: image deploys and rollbacks, Git builds (built on the panel host, then streamed to the server with `docker save`/`docker load`), databases, backups and restores, compose stacks, domains and certificates, the HTTP cache, logs and the live tail, the web terminal, metrics and alerts, and Docker prune.

Routing changes made while an agent is offline are stored and written to the server when it reconnects. Deploys to a server whose agent is offline fail immediately with `remote_unavailable`.

Git-sourced compose stacks are cloned on the panel, then the tracked files are copied to the server (the `.git` directory stays on the panel). Build contexts and files the compose file mounts from the repository are included. Files already on the server that are not part of the repository are kept. A checkout larger than 512 MiB is refused.

## Build hosts

For the setup and panel workflow, see the [build host operator guide](build-hosts.md).

Migration 0028 adds opt-in builder settings to servers, independent build placement
to services, builder attribution to queued jobs, and a build-cache inventory. All
existing services keep `panel` placement; this does not move builds or
enable builders automatically. Builder enrollment, HTTP/OpenAPI/MCP management and
local status are available, together with the Builders page and translated service
build controls.

Agents report architecture (`x86_64`/`aarch64`) and whether `docker buildx version`
and `nixpacks --version` succeed in Hello. Build commands use the existing outbound
mTLS Session/Channel, not a new inbound listener:

- `build_start`: upload tracked files with ExtractTree to
  logical path `builder/workspaces/<job_id>/src` first. Builder uploads are routed
  below the agent data directory, not the deployment node directory. Supply the shared
  build plan, service/job IDs, opaque branch cache key, up to two fallback keys
  (branch then default branch), cache byte limit, timeout and optional image test.
- The start reply only acknowledges acceptance. The output Channel carries one JSON
  `log` event per frame and a final `finished` event (`succeeded`, `failed`,
  `cancelled`, `timed_out`). A stream ending without `finished` is not success.
- `build_cancel` cancels an active job, returning false for an unknown/completed job.
  Dropping the log Channel also cancels. Accepted jobs clean up their workspace;
  cancellation and timeout kill the process group and remove any image-test container.
- `build_cache_usage` optionally filters by service. `build_cache_prune` deletes
  all matching caches or evicts least-recently-used entries to a byte limit. Key
  pruning requires a service. Active services are protected; explicit pruning of an
  active service returns busy. Timestamps in agent cache records are Unix milliseconds.

Linux builders need Docker Buildx with a local image store. Dockerfile/static builds
use the same core arguments as panel builds plus `buildx build --load`. Nixpacks
generates its `.nixpacks` Dockerfile/assets with `--out`, then the same Buildx path
builds the image. The optional test runs `docker run --rm <image> sh -c <command>`
with Docker's default networking; nonzero exit fails the build. It is not sandboxed
from networks. Build hosts must be trusted with the source, build arguments and Docker
daemon; raw build-argument values are redacted even across output chunks, but—as with
panel builds—Dockerfile/Nixpacks authors must avoid persisting secrets in image layers.

Local cache imports try the requested key, branch and default branch in order,
deduplicated. Exports go to staging and atomically replace the key only after the
build and image test succeed. The node's configured Docker socket is honored. Linux
atomic directory exchange keeps the old cache on failure; LRU runs after every job.
Concurrent builds of different services are supported; overlapping builds of the
same service are rejected to protect cache imports/exports. Global pruning skips
active services, so a byte limit can be temporarily exceeded until those jobs finish.
The configured default limit is 20 GiB. Cache storage is separate from Dymini Panel's HTTP
cache.

The panel dispatches jobs concurrently. Panel builds share the configured panel
build-concurrency limit and load guard; builders have independent `builder_max_jobs`
limits. Pausing all builds still holds every new job. A draining builder accepts no
new jobs, but running jobs finish. Claims reserve slots transactionally across workers.

Placement is recorded in deployment logs. `builder` uses only the chosen builder;
`auto` considers enabled, connected builders that are not paused/draining, have a free
slot, match the deploy target's architecture and support Buildx (and Nixpacks when
needed). Local deploys use the panel's architecture. Cache affinity for the service
ranks first, then fewer active jobs, then builder name. No eligible builder either
falls back to panel execution or stays queued with a visible waiting reason, according
to the service fallback. Queued jobs remain cancellable.

The panel holds Git credentials, clones the source, discovers its default branch and
sends only tracked files, preserving executable scripts without special permission
bits. Builds restore opaque branch/default-branch cache keys and run the configured
image test on either panel or builder. Builder logs are redacted and flushed to
deployment logs about once a second. Cancellation sends `build_cancel`; timeout also
cancels the remote job. `build_workspace_remove` cleans interrupted uploads and stale
workspaces before a retry. Migration 0029 records the single allowed disconnect retry;
a second disconnect fails clearly, while the retry re-runs placement.

After a successful builder run the panel refreshes its cache inventory, relays the
image save/load streams directly into the deploy target, and uses the existing
health-checked rollout. It does not load remote-target images into the panel's Docker
daemon. Builder-only image tags are removed afterwards without force; cleanup failures
are logged. When builder and deploy target are the same server, the image stays there
for the application instead of being copied or removed.

### Real-Docker executor smoke

On Linux with Docker and Buildx, run this opt-in test before releasing builder
changes. It pulls BusyBox and BuildKit images and starts a private BuildKit
instance. It does not prune the default/shared Docker build cache.

```sh
stamp=$(date +%s)
BUILDX_BUILDER="dymini-panel-builder-smoke-$stamp" \
DYMINI_PANEL_BUILDER_SMOKE_DIR="$HOME/dymini-panel-builder-smoke-$stamp" \
cargo test -p dymini-panel-agent --test builder_docker -- --ignored --nocapture
```

The evidence directory must not exist. JSON event logs persist there, while test
images and the dedicated BuildKit instance are removed at completion. The smoke
checks loaded images, atomic cache replacement, cold-BuildKit local cache imports,
default-branch fallback, passing/failing image tests, cancellation/timeout of both
Dockerfile RUNs and image tests, workspace cleanup and exact byte-accounted pruning.
RUN/test liveness is measured using host PIDs and HTTP heartbeats to an ephemeral
listener bound only to the Docker bridge gateway, not a public interface.

## Security

- Enrollment only runs over `https://` (plain `http://` only to the same machine), and the agent checks the returned CA against the `--ca-sha256` fingerprint from the join command before saving it.
- The join token is stored only as a SHA-256 hash, works once, and expires after an hour. Enrollment is rate limited and audited (`server.enroll`).
- The agent's private key never leaves the server. The panel signs the request's public key with its own certificate authority, whose key is sealed with the master key.
- Every gRPC call requires a client certificate from that CA, and the certificate's fingerprint must match the server row. Deleting a server, or enrolling it again, disconnects the old session and rejects its certificate.
- The agent verifies the panel against the same CA using the name `dymini-panel`, so the panel can be reached by IP address.
- The panel only sends `NodeDriver` operations. File writes are confined to `/var/lib/dymini-panel` on the server, with mode 0600.

## Telemetry and outages

The agent samples host and container stats every 10 seconds and follows the output of Dymini-Panel-managed containers, `dymini-panel-traefik`, and `dymini-panel-cache`. It keeps doing this while the panel is unreachable and holds up to 20,000 log lines (4 MiB) and one hour of stats in memory. When the buffer is full the oldest items are dropped; the server row counts them (`telemetry_dropped`). Batches are resent until the panel acknowledges them. Log positions are saved only after acknowledgement, so restarting the agent re-reads missed lines from Docker.

The agent reconnects with backoff from 1 to 30 seconds. The panel marks the server offline when the session ends and records `server.online` and `server.offline` events, which the alert rules can notify on.

## Panels reachable only over HTTPS

A panel behind a Cloudflare Tunnel (see [cloudflare.md](cloudflare.md)) or
another HTTPS-only reverse proxy cannot offer the raw agent port. Everything
the agent needs then runs on the panel's HTTPS address under `/mcp`, the path
family that Cloudflare Access's agent bypass keeps open:

- enrollment posts to `/mcp/api/v1/agent/enroll`,
- the installer and agent binary download from `/mcp/install-agent.sh` and
  `/mcp/downloads/...`,
- the session dials a WebSocket at `/mcp/api/v1/agent/tunnel`, which the panel
  pipes to its internal agent listener. The mTLS handshake happens inside the
  socket, so the tunnel never sees credentials and `DYMINI_PANEL_AGENT_BIND` can stay
  bound to the panel host (or a private interface) only.

Agents enrolled before the panel closed its ports keep using the direct
connection; they pick up the tunnel automatically the first time the direct
dial fails. No flag is needed on either side.

## Panel settings

| Variable | Default | Meaning |
| --- | --- | --- |
| `DYMINI_PANEL_AGENT_BIND` | `0.0.0.0:7443` | gRPC listener for agents, or `off` |
| `DYMINI_PANEL_AGENT_URL` | panel host + agent port | Address agents are told to dial (`https://` only), for example when a proxy or NAT changes the port |
| `DYMINI_PANEL_DIST_DIR` | `/usr/share/dymini-panel/dist` | Agent binaries served at `/downloads/dymini-panel-agent-<arch>-unknown-linux-musl` with a `.sha256` file |

## Agent files and commands

### Install a build host

Create a builder with `POST /api/v1/builders` and run its returned command. It uses
the usual one-use enrollment token and CA fingerprint plus `--builder --no-edge`.
Builder mode implies no edge containers: no Traefik/cache containers are started.
The server initially has **no services assigned**; explicitly assigning a workload
to it later makes it a deployment target too. There is no separate deploy-role flag.

On Debian/Ubuntu, `--builder` installs missing Docker from Docker's official apt
repository and installs `docker-buildx-plugin` if Buildx is missing. Existing Docker
must be running. Other distributions need Docker and Buildx installed manually
before running the installer. `--with-nixpacks` additionally runs the official
Nixpacks installation script with `NIXPACKS_VERSION=1.41.0` pinned and verifies it.
These installation paths require network access; package conflicts stop with an
actionable error rather than removing an existing container runtime.

The installer writes `{"builder":true}` to `/var/lib/dymini-panel-agent/config.json`.
Alternatively set it there or run `dymini-panel-agent run --builder`. Restart the service
after changing config. Ordinary agents do not advertise builder capabilities or
accept builder workspace uploads unless builder mode is enabled. Build workspaces
and caches live in `/var/lib/dymini-panel-agent/builder/`; deployment files remain under
`/var/lib/dymini-panel/`. The installer prints status and fails clearly if connection
has not succeeded within 30 seconds. An existing enrollment is preserved on reruns.

`dymini-panel-agent status --json` reads the local credential-free `status.json`, maintained
with atomic replacement and mode 0600. It includes panel/server identity, enrollment,
connection, last connect/error, builder mode, Buildx/Nixpacks, architecture, active
job IDs and aggregate cache bytes. A heartbeat updates it every five seconds;
connection changes are recorded immediately. A snapshot older than 30 seconds is
disconnected, so a killed service cannot remain online indefinitely. Both text and
JSON status exit nonzero when unenrolled or disconnected. Use `--data-dir` when
running a custom installation. Reading status does not probe the panel or run Docker.

### Manage builders through HTTP or MCP

Builder management uses `catalog:read`/`catalog:write`, not a new MCP scope:

- `GET /api/v1/builders` returns `builders` (all remote server candidates, including
  disabled ones so existing servers can be enabled) and up to 200 unclaimed
  `waiting_jobs`. Each builder includes live session state, last seen/version/arch,
  capabilities, settings, active jobs, explicitly targeted queued jobs, and last
  reported cache bytes versus its limit. Auto jobs are not reserved until claim.
- `POST /api/v1/builders` accepts `name` and optional `description` and returns
  `server`, `join_token`, `join_command` and `expires_at`. Treat tokens as secrets.
- `PATCH /api/v1/builders/{id}` accepts optional `enabled`, `max_jobs` (1–32),
  `paused`, `draining`, and `cache_limit_bytes` (non-negative). It can enable builder
  mode on an existing remote server, but the agent must also be installed/configured
  with builder mode. Disabling or draining stops new claims, not active jobs.
- `GET /api/v1/builders/{id}/cache` reads the last reported per-service/key catalog,
  including while offline. `DELETE` on the same path clears caches, optionally
  filtered by `service_id` and/or `key`. Omit both to clear all inactive caches.
  `POST /api/v1/builders/{id}/cache/prune` applies LRU down to the configured limit.
  Mutations require a connected agent; active-service caches are protected. Cache
  catalog replacement shares the build-completion lock so stale refreshes cannot
  resurrect deleted entries. The result reports reclaimed/remaining bytes and entries.
- `GET /api/v1/builders/{id}/jobs` returns up to 100 recent jobs, prioritizing all
  running jobs, including unclaimed jobs targeting this builder. Deployment IDs link
  to deployment logs and waiting reasons. Standard build cancellation still applies.

`GET/PUT /api/v1/services/{id}/source` uses the existing `source` MCP scope and now
includes a `build` object: `placement` (`panel`, `auto`, `builder`), `builder_id`,
`fallback` (`panel`, `wait`), nullable `test_command`, and positive `timeout_secs`.
Omitting `build` in PUT preserves settings. Supplying it replaces the object;
omitted fields default to panel placement/fallback, no builder/test, and 3600 seconds.
Builder placement requires an existing, builder-enabled `builder_id`; any selected
builder ID is validated even with other placement modes. Null clears the optional
test command or builder ID within a supplied object. Buildx is required; Nixpacks
also requires the Nixpacks capability. Settings do not cause a build automatically.

| Path | Content |
| --- | --- |
| `/usr/local/bin/dymini-panel-agent` | The binary |
| `/var/lib/dymini-panel-agent/` | Identity files, `config.json`, `status.json`, `log-cursors.json` (mode 0600, directory 0700), and `builder/` workspaces/caches |
| `/var/lib/dymini-panel/` | Files the panel writes: Traefik routes and certificates, cache rules, compose stacks |
| `/etc/systemd/system/dymini-panel-agent.service` | The unit |

```
dymini-panel-agent enroll --server <panel URL> --token <token> [--ca-sha256 <hex>]
                    [--data-dir /var/lib/dymini-panel-agent]
dymini-panel-agent run [--data-dir /var/lib/dymini-panel-agent] [--node-dir /var/lib/dymini-panel]
                 [--docker-socket <path>] [--cache-admin-url <url>] [--builder]
dymini-panel-agent status [--data-dir /var/lib/dymini-panel-agent] [--json]
dymini-panel-agent --version
```

`journalctl -u dymini-panel-agent` shows the agent's JSON log. The same lines appear in the panel's log explorer under source `agent`.

## Building the agent

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools
CC_x86_64_unknown_linux_musl=musl-gcc \
  cargo build --release -p dymini-panel-agent --target x86_64-unknown-linux-musl
```

The result is a static-pie binary of about 11 MB.
