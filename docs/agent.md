# Remote servers and the agent

A remote server runs `rudder-agent`, a single static binary. The agent dials the panel, runs the panel's Docker calls on its own machine, and ships that machine's logs and metrics back. The panel never connects to the server, so servers behind NAT or a firewall work as long as they can reach the panel.

```
panel :7443 (gRPC, mTLS)  ◀── dial out ──  rudder-agent ── Docker Engine
                                             └─ rudder-traefik, rudder-cache, apps
```

## Add a server

1. In **Servers**, add a server with kind `remote`. The panel shows a one-line command with a one-time join token that expires in an hour.
2. Run the command on the server as root:

   ```bash
   curl -fsSL https://panel.example.com/install-agent.sh | sudo sh -s -- \
     --server https://panel.example.com --ca-sha256 3f6c... --token join_...
   ```

   The panel URL must use `https://`; `http://` is refused unless it points at `localhost`, `127.0.0.1`, or `[::1]`, because enrollment receives the certificate authority the agent trusts from then on. `--ca-sha256` is the SHA-256 fingerprint of the panel's agent CA, and the panel puts it in the command it shows. If the CA the panel returns has a different fingerprint, enrollment stops and nothing is saved.

   The installer needs Docker, systemd, `curl`, and `sha256sum`. It:
   - downloads `rudder-agent` for the machine's architecture from the panel (`/downloads/...`), or from the GitHub release when the panel has no bundled copy, and checks its SHA-256;
   - runs `rudder-agent enroll`, which creates a private key on the server and exchanges the token and a certificate request for a client certificate;
   - creates the `rudder-proxy` network and starts `rudder-traefik` (ports 80 and 443, JSON access log) and `rudder-cache` (admin port on `127.0.0.1:9090` only);
   - installs and starts the `rudder-agent` systemd unit.
3. The server turns **online** within a few seconds. The Servers page shows the agent version, hostname, OS, Docker version, and last contact.

Options: `--version v1.2.3` pins the GitHub release, `--email` sets the Let's Encrypt account email, and `--no-edge` skips Traefik and the cache. Running the installer again upgrades the binary and keeps the existing identity. To move a server to a new identity, use **Join command** on the server row to get a fresh token, delete `/var/lib/rudder-agent` on the server, and rerun the command. The old certificate stops working as soon as the new one is issued.

## Agent update verification

Before checking a downloaded agent's version, the agent flushes and syncs the staged binary and closes its writable handle. If another starting process briefly inherited that handle, Linux can still report `Text file busy`. The version check retries only that spawn error, at most 10 attempts with 100 ms between attempts (900 ms of backoff), within the existing 15-second check timeout. Other execution errors fail immediately.

## What works on a remote server

Everything the local server supports goes through the agent: image deploys and rollbacks, Git builds (built on the panel host, then streamed to the server with `docker save`/`docker load`), databases, backups and restores, compose stacks, domains and certificates, the HTTP cache, logs and the live tail, the web terminal, metrics and alerts, and Docker prune.

Routing changes made while an agent is offline are stored and written to the server when it reconnects. Deploys to a server whose agent is offline fail immediately with `remote_unavailable`.

Git-sourced compose stacks are cloned on the panel, then the tracked files are copied to the server (the `.git` directory stays on the panel). Build contexts and files the compose file mounts from the repository are included. Files already on the server that are not part of the repository are kept. A checkout larger than 512 MiB is refused.

## Build-host foundation (not yet scheduled by the panel)

Migration 0028 adds opt-in builder settings to servers, independent build placement
to services, builder attribution to queued jobs, and a build-cache inventory. All
existing services keep `panel` placement; this foundation does not move builds or
enable builders automatically. Builder enrollment flags, scheduling, HTTP/OpenAPI/MCP
management, UI and its translations follow in later slices.

Agents report architecture (`x86_64`/`aarch64`) and whether `docker buildx version`
and `nixpacks --version` succeed in Hello. Build commands use the existing outbound
mTLS Session/Channel, not a new inbound listener:

- `build_start`: upload tracked files with ExtractTree to
  `builder/workspaces/<job_id>/src` under the node directory first. Supply the shared
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
The configured default limit is 20 GiB. Cache storage is separate from Rudder's HTTP
cache. Image return and deployment integration are part of the later scheduler slice.

## Security

- Enrollment only runs over `https://` (plain `http://` only to the same machine), and the agent checks the returned CA against the `--ca-sha256` fingerprint from the join command before saving it.
- The join token is stored only as a SHA-256 hash, works once, and expires after an hour. Enrollment is rate limited and audited (`server.enroll`).
- The agent's private key never leaves the server. The panel signs the request's public key with its own certificate authority, whose key is sealed with the master key.
- Every gRPC call requires a client certificate from that CA, and the certificate's fingerprint must match the server row. Deleting a server, or enrolling it again, disconnects the old session and rejects its certificate.
- The agent verifies the panel against the same CA using the name `rudder-panel`, so the panel can be reached by IP address.
- The panel only sends `NodeDriver` operations. File writes are confined to `/var/lib/rudder` on the server, with mode 0600.

## Telemetry and outages

The agent samples host and container stats every 10 seconds and follows the output of Rudder-managed containers, `rudder-traefik`, and `rudder-cache`. It keeps doing this while the panel is unreachable and holds up to 20,000 log lines (4 MiB) and one hour of stats in memory. When the buffer is full the oldest items are dropped; the server row counts them (`telemetry_dropped`). Batches are resent until the panel acknowledges them. Log positions are saved only after acknowledgement, so restarting the agent re-reads missed lines from Docker.

The agent reconnects with backoff from 1 to 30 seconds. The panel marks the server offline when the session ends and records `server.online` and `server.offline` events, which the alert rules can notify on.

## Panel settings

| Variable | Default | Meaning |
| --- | --- | --- |
| `RUDDER_AGENT_BIND` | `0.0.0.0:7443` | gRPC listener for agents, or `off` |
| `RUDDER_AGENT_URL` | panel host + agent port | Address agents are told to dial (`https://` only), for example when a proxy or NAT changes the port |
| `RUDDER_DIST_DIR` | `/usr/share/rudder/dist` | Agent binaries served at `/downloads/rudder-agent-<arch>-unknown-linux-musl` with a `.sha256` file |

## Agent files and commands

| Path | Content |
| --- | --- |
| `/usr/local/bin/rudder-agent` | The binary |
| `/var/lib/rudder-agent/` | `key.pem`, `cert.pem`, `ca.pem`, `agent.json`, `log-cursors.json` (mode 0600, directory 0700) |
| `/var/lib/rudder/` | Files the panel writes: Traefik routes and certificates, cache rules, compose stacks |
| `/etc/systemd/system/rudder-agent.service` | The unit |

```
rudder-agent enroll --server <panel URL> --token <token> [--ca-sha256 <hex>]
                    [--data-dir /var/lib/rudder-agent]
rudder-agent run [--data-dir /var/lib/rudder-agent] [--node-dir /var/lib/rudder]
                 [--docker-socket <path>] [--cache-admin-url <url>]
rudder-agent --version
```

`journalctl -u rudder-agent` shows the agent's JSON log. The same lines appear in the panel's log explorer under source `agent`.

## Building the agent

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools
CC_x86_64_unknown_linux_musl=musl-gcc \
  cargo build --release -p rudder-agent --target x86_64-unknown-linux-musl
```

The result is a static-pie binary of about 11 MB.
