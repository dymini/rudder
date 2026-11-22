# Plugins

Plugins are curated compose stacks whose images are pinned to verified GitHub release
descriptors. **Deploy → Plugins → Dockside → Check for updates → Install** is the first
supported integration. Multiple installations of the same plugin are allowed, including
in different environments or servers.

## Install Dockside

Choose a project, environment, server, service name, domain and verified version. Rudder
generates a 48-character authentication token when left empty and shows it once after
installation. Secrets are stored encrypted, just like stack environment variables.

Paste an optional `dockside.toml` into the multi-line field. Rudder base64-encodes it into
`DOCKSIDE_CONFIG_B64`. Empty means the example configuration shipped in the image. Set
optional R2 credentials for blob write-through, or Rudder/Cloudflare credentials for
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
`rudder-plugin.json`: schema 1, matching slug and tag version, matching health path/port,
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

The image variable is managed by Rudder and rejected in user-supplied install values.
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
