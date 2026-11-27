# Plugins

## Dashboard protection and agent paths

Dockside plugin installs default to **Dymini Panel Access enabled**. The plugin settings
drawer includes a protection toggle. Configure Access sign-in (portal, Turnstile,
and email or Google) before opening the dashboard. New policies allow only the
original panel owner's email and create that Access identity if absent; they do
not make the identity an Access administrator or unblock a blocked identity.
Existing policies retain their people/department/rank rules. Updates synchronize
the manifest's path bypasses and the saved toggle, including older installs.
Every route publication (including startup and tunnel synchronization) also
reconciles existing installs, so upgrading does not require reinstalling or
updating Dockside to apply protection. The saved plugin toggle is authoritative;
changing the service's Access toggle updates it too. Plugin bypass paths remain
manifest-managed. Existing allow rules are preserved. Publication failures are
returned by the API and recorded in the plugin's `last_error`; retry publication
after resolving the error.
An offline server cannot apply proxy changes until connected; API errors report
publication failure rather than pretending the live proxy was updated. A stopped
offline install can store its policy; protection is published before starting it.

The manifest `access.bypass_paths` is a list of `{path, minimum_version?}` entries.
Every other path, including `/`, unknown future paths and `/v1/queue/drain`, stays
behind Access. This is a general service-policy capability, not an edge-gate
Dockside exception. `PUT /api/v1/access/services/{id}` accepts an optional
`bypass_paths` array; omitting it preserves existing bypasses. Prefixes match
complete path segments, never lookalike names, encoded paths or dot segments.
Shared hostnames bypass only when every enabled Access policy agrees. Use a
dedicated hostname for a plugin, and do not expose its container port directly.
Edge security/rate limits still apply to bypassed requests.

**Paths bypassing Access:** `/v1/blobs`, `/v1/revisions`, `/v1/import`,
`/v1/changesets`, `/v1/leases`, `/v1/sequences`, `/v1/deployments`, `/v1/releases`,
and `/v1/rollbacks`, including their subpaths. They still require
`DOCKSIDE_AUTH_TOKEN`. `/v1/health` and subpaths join that list only on Dockside
**0.2.2 or later**, where health also requires the bearer token. Older Dockside
health checks remain Access-protected externally; run CLI health internally or
upgrade for external CLI health. No dashboard/UI path bypasses Access. With the
toggle off, dashboard HTML becomes public again; API bearer authentication does
not change. Descriptor images stay digest-pinned, and the maintenance changes do
not publish a Dockside release automatically: release/tag 0.2.2 before selecting it.

## Backup management and recovery

Plugin settings show **all jobs attached to the install's service**, including
disabled/replaced jobs and restore safety jobs. Each includes destinations,
schedule/timezone, retention and recent runs with size, duration, status and logs.
Run now, edit schedule/keep-last (preserving other retention settings), or load
more history. General backup APIs provide these operations under `backups`;
the plugin Access toggle uses `stacks`. All are MCP tools.

Normal Dockside backups stop the service for the entire volume copy. On 0.2.2+,
the manifest's versioned `backup.pre_hook` runs `dockside-server checkpoint`
first. It flushes WAL and fails on a busy checkpoint; the subsequent stop is
still essential. Older versions use a cold whole-volume copy, including WAL/SHM.
The metadata database and CAS must be copied together. Hooks are skipped for an
already stopped service; it remains stopped afterward.

`POST /api/v1/plugin-installs/{id}/restore` with `{"run_id":"…"}` accepts a
successful/partial volume backup of **this install's data volume**. It returns
202 and the install's `restoring` status; poll the plugin catalog for progress,
`last_error`, and `safety_run_id`. Service-targeted generic restores reject plugin
targets so the normal UI/API cannot accidentally skip safety orchestration.

1. Claim the install and stop the container. Plugin backups/updates are serialized.
2. Create a separate, disabled, unfiltered full-volume safety job using the source
   job's destinations and encryption. Require its backup to succeed before writing.
   Its independent retention does not prune the selected source run.
3. Clear the stopped data volume, including stale SQLite side files, and restore
   the selected archive. Extraction is replacement, not an overlay.
4. Start and health-check using an in-container bearer-authenticated probe.
5. On restore/start/health failure, stop, clear, restore the safety run, start and
   check health again. `rolled_back` and the reason remain visible. If rollback
   also fails, the error identifies the safety run for operator recovery.

Safety jobs remain disabled and their run stays available for manual management;
they are not scheduled or automatically deleted with another job's retention.
If a backup/restore is still active or its completion cannot be established,
Dymini Panel refuses to clear or restart concurrently. Background orchestration runs in
the panel process: a panel crash requires operator recovery from the retained
safety run rather than promising crash-resumable transactions. Avoid deployment,
manual container actions or direct volume writes during recovery. Real Docker,
S3 and Access-provider connectivity must be verified in your deployment.

Plugins are curated compose stacks whose images are pinned to verified GitHub release
descriptors. **Deploy → Plugins → Dockside → Check for updates → Install** is the first
supported integration. Multiple installations of the same plugin are allowed, including
in different environments or servers.

## Install Dockside

Choose a project, environment, server, service name, domain and verified version. Dymini Panel
generates a 48-character authentication token when left empty and shows it once after
installation. Secrets are stored encrypted, just like stack environment variables.

Paste an optional `dockside.toml` into the multi-line field. Dymini Panel base64-encodes it into
`DOCKSIDE_CONFIG_B64`. Empty means the example configuration shipped in the image. Set
optional R2 credentials for blob write-through, or Dymini Panel/Cloudflare credentials for
Dockside release targets. Optional secrets are not generated. Verification defaults to on.

Dockside runs on port 7420 behind the domain route; no host port is published. Its compose
service drops all capabilities, uses no-new-privileges, and limits memory, CPU and PIDs.
Its persistent `data:/data` volume includes SQLite history, CAS, Git mirror and toolchains.

After installation use the service's **Environment** tab for configuration changes. When
editing there, encode TOML yourself into `DOCKSIDE_CONFIG_B64`; the install form's text
encoding is not applied to arbitrary environment edits. Do not change `PLUGIN_IMAGE`
manually if you want plugin version tracking to remain accurate.

## Backups and updates

Select one or more S3 destinations during installation, or choose **Set up backup** later.
The default is `@daily`, retaining the last seven backups. The job archives the actual
compose volume `<project>_data`, stopping the service for SQLite consistency. Replacing
the plugin's backup job disables the old job's schedule; its backup history remains.

**Check for updates** fetches stable, non-draft GitHub releases and verifies each
`dymini-panel-plugin.json`: schema 1, matching slug and tag version, matching health path/port,
and an image pinned with a SHA-256 digest. This checks descriptor consistency, not a
cryptographic publisher signature. Trust comes from the curated manifest's GitHub repo
and HTTPS. Release notes are bounded to 8,000 characters per release. There is no periodic
check: use the manual action. The last check, errors and verified releases are cached in
the database; a transient GitHub error preserves the previously verified cache.

The page shows newer release notes in newest-first order. An update runs the attached
backup first and waits for full success (partial backups do not qualify), then redeploys
the pinned image and checks its HTTP health endpoint. Failure restores the previous image,
redeploys and checks health again. Successful recovery is `rolled_back`; failed recovery
is `failed`, with the error shown on the page. Image rollback does **not** restore data or
undo database migrations: use your backup to recover incompatible data changes.

Updates are asynchronous; refresh the catalog to observe `updating`, `installed`,
`rolled_back`, or `failed`. Concurrent plugin operations return 409. A default update must
be newer; explicitly specifying a verified `version` through the API allows a reinstall
or downgrade. Updates without a backup job are allowed but not recommended.
If the panel restarts mid-operation, the install is marked `failed` to release its lock.
Inspect the stack's image/environment and deployment before retrying: restart recovery
does not claim an interrupted operation succeeded or that an image rollback completed.

## API and MCP

All routes use the existing **stacks** scope. Mutations need write permission and are audited.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/v1/plugins` | Catalog, installs, latest versions and last checks |
| GET | `/api/v1/plugins/{slug}` | Manifest, variables, compose and cached releases |
| POST | `/api/v1/plugins/{slug}/check` | Refresh release discovery |
| POST | `/api/v1/environments/{id}/plugins/{slug}/install` | Install with `name`, `server_id`, `values`, optional `start`, `version`, `backup` |
| POST | `/api/v1/plugin-installs/{id}/update` | Update with `{}` or explicit `{ "version": "0.2.0" }` |
| POST | `/api/v1/plugin-installs/{id}/backup` | Attach/replace backup with `destination_ids`, optional `schedule`, `keep_last` |

The image variable is managed by Dymini Panel and rejected in user-supplied install values.
Install returns the service, domain, deployment, generated secrets and install record.
Without a verified cached release, install/update returns 409; check releases first.

## Uninstall

Delete the stack service using the normal service deletion action. Its plugin install row
is removed automatically. Volume removal follows the stack deletion option; keep volumes
if you need the data. Review retained backup jobs and objects separately.

## Adding a plugin

Add a YAML manifest under `plugins/`; files are embedded in the server at build time.
Use template fields plus `repo`, optional `descriptor_asset`, `image_variable`, `data_volume`
and `health_path`. Declare the image variable, declare the data volume in compose, and
include a healthcheck. Text variables require `kind: text`, `encoding: base64`, and may
set `env_name` to the encoded environment key. Existing templates keep their behavior.
