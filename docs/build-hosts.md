# Build hosts

A trusted machine at home or elsewhere can build Git applications for Rudder.
The panel clones the repository, sends its tracked tree over the existing outbound
mTLS agent connection, receives the built image and runs the normal health-checked
deployment. Dockerfile, static and Nixpacks builds share the same build planning.
Existing services keep building on the panel until their placement is changed.

## Add and enroll a machine

1. Open **Builders**, beside **Servers**, and choose **Add build host**.
2. Enter a server name and copy the enrollment command. Run it as root on the host:

   ```sh
   curl -fsSL https://panel.example/mcp/install-agent.sh | sudo sh -s -- \
     --server https://panel.example --ca-sha256 <fingerprint> --token <one-use-token> \
     --builder --no-edge
   ```

   The command also works when the panel is only reachable through a
   Cloudflare Tunnel with all inbound ports closed: `/mcp` is the path family
   the agent bypass keeps open, and the agent session falls back to a
   WebSocket on the panel's HTTPS address when the agent port is unreachable.

   Use the exact command Rudder issues, including its CA fingerprint. The token is
   secret, works once and expires in one hour. Do not paste it into tickets or logs.
3. Append `--with-nixpacks` if the machine should build Nixpacks applications. The
   installer uses the official script with a pinned version (currently 1.41.0).
4. Run `sudo rudder-agent status`. The Builders page polls automatically; an enrolled
   agent becomes online without reloading the page.

The installer needs Linux/systemd. On Debian/Ubuntu it can install missing Docker
from Docker's official apt repository and install `docker-buildx-plugin`. Other
distributions need Docker and Buildx installed manually. Existing Docker must be
running. Builder mode implies `--no-edge`: no Traefik/cache edge containers start.

A new build host has no workloads assigned. Assigning a workload later also makes
it a deployment target; there is no separate deploy-role switch. To reuse a remote
server, choose **Enable builder mode** on Servers, obtain its **Join command**, and
rerun the installer with `--builder --no-edge`. Existing enrollment is preserved.
Enabling the panel setting alone does not enable the agent: both sides must opt in.
All remote candidates remain listed, including disabled ones.

## Status and settings

```sh
sudo rudder-agent status
sudo rudder-agent status --json
sudo rudder-agent status --data-dir /custom/agent/data --json
journalctl -u rudder-agent
```

Status reports the panel URL, server ID, enrollment, connection, last connection and
error, builder mode, Buildx/Nixpacks, architecture, running job IDs and cache usage.
It reads an atomic local snapshot, not a new network probe. Snapshots older than
30 seconds count as disconnected. Text and JSON status return nonzero if unenrolled
or disconnected. The running service updates the file every five seconds and on
connection changes. Credentials are never included.

**Manage build host** exposes enabled mode, maximum jobs (1–32), paused, draining
and the cache byte limit (displayed in GiB). Disabling, pausing or draining stops
new claims, not running jobs. Drain before maintenance, wait for running jobs to
finish, then stop the agent. Queue counts on each host count jobs explicitly
targeting that host; auto jobs remain in the shared waiting list until claimed.
Jobs link to the service's Deployments tab, including logs and waiting reasons.

## Service placement and fallback

In an application's **Source** section, configure **Build host settings**, then
save the source. Saving settings does not queue a build; use **Build now** or a Git
webhook afterward.

- **Panel**: keep the existing panel-host build path and its load guard/concurrency.
- **Automatic build host**: consider enabled, connected hosts that are not paused
  or draining, have a free slot, support Buildx (and Nixpacks when needed), and match
  the deployment target's architecture. Cache affinity ranks first, then fewer
  active jobs, then name. Local deployment targets use the panel architecture.
- **Specific build host**: consider only the selected enabled host. The same
  architecture, capability and capacity rules apply.
- **Build fallback**: build on the panel if no host is eligible, or **Wait for a
  build host** to leave the job queued. Queued deployments show the latest waiting
  reason without opening their logs. Existing cancellation still applies.
- **Image test command**: optional `sh -c` command inside the built image, with
  Docker's default network. A nonzero exit stops deployment on either placement.
- **Build timeout**: positive seconds shared by building and the optional image
  test, default 3600. Timeout/cancellation stops the build process tree.

API validation reasons are shown directly. A selected host must exist and have
builder mode enabled. A disconnect during building requeues once and repeats
placement; a second disconnect fails clearly.

## Cache keys and pruning

Builder workspaces and caches live under `/var/lib/rudder-agent/builder/`, or
`<agent-data-dir>/builder/` with a custom agent data directory. They are separate
from deployment-node files. Buildx exports a local cache per service and opaque
branch key, with branch then default-branch fallback imports. Successful cache
exports replace the previous cache atomically. Failed builds/tests do not replace
a good cache. Nixpacks generates the Dockerfile before the same Buildx path.

The default limit is 20 GiB. Automatic LRU runs after builds. The per-host cache
table shows service, key, size and last use; **Clear** targets one entry, **Clear
all caches** clears all inactive entries, and **Prune to limit** evicts least-recently
used entries down to the **saved** limit. All three ask for confirmation. Active
service caches are protected and may keep usage above the limit temporarily.
Setting zero evicts inactive caches at the next prune. Offline entries are the
last reported catalog; cache mutations require a connected builder.

## Security and network requirements

- The builder receives source and **build-secret arguments**, and runs arbitrary
  Dockerfile/test commands. Only enroll machines and service authors you trust.
  Treat the Docker socket as root-equivalent. Image tests use the default network,
  not an isolated sandbox; consider the host's network access and firewall rules.
- The agent connects **outbound only**, using the existing mTLS gRPC session and
  channels (usually the panel's port 7443, or the WebSocket tunnel on the
  panel's HTTPS address when no agent port is open). No inbound builder
  listener or public registry is required. Image save/load streams are relayed
  through the panel.
- Repository credentials remain on the panel, which clones and transfers tracked
  files. Build arguments still reach the builder. Log redaction is defense in
  depth, not a guarantee against malicious build commands printing secrets.
- Protect enrollment commands and the agent data directory. Do not share private
  keys, copy real enrollment commands into screenshots, or expose untrusted builds
  to production credentials. The panel's catalog/source MCP scopes control these
  same settings and cache operations.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Host stays offline | Run `rudder-agent status` and inspect `journalctl -u rudder-agent`. Check the panel URL, agent port, CA fingerprint, outbound firewall and clock. |
| Online but Buildx unavailable | Enable builder mode in both panel and agent config; rerun the builder installer. Check `docker info` and `docker buildx version`, then restart the agent. |
| Nixpacks jobs wait | Install with `--with-nixpacks`, check `nixpacks --version`, then restart the agent to refresh capabilities. |
| Queued with free slots | Check deployment architecture, selected host, enabled/paused/draining state, strategy capabilities and global build pause. Read the queued deployment's visible waiting reason. |
| Builds unexpectedly run on panel | Check placement and panel fallback. Set fallback to wait if panel execution is not acceptable. |
| Cache cannot be cleared | Connect the host; active caches are protected. Save the limit before LRU pruning. Retry after running jobs finish. |
| Disk still high | The cache limit does not include all Docker layers/images or deployment volumes. Inspect Docker usage separately; do not delete running workloads' data. |

For protocol, enrollment and file-layout details, see [agent operations](agent.md).
