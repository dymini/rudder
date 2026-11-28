# Panel backup and disaster recovery

The panel's state is its PostgreSQL database plus the master key that decrypts the
secrets stored in it (environment values, database passwords, registry and S3
credentials, deploy keys, certificates, the agent CA). Everything under
`/var/lib/dymini-panel` — Traefik dynamic config, certificate files, cache rules, compose
files — is regenerated from the database when the panel starts and when services
are deployed.

## What a panel backup contains

A panel backup is a tar archive, compressed with zstd and always encrypted with
the `RDRENC01` envelope (Argon2id key from a passphrase, AES-256-GCM frames):

| Entry | Content |
| --- | --- |
| `manifest.json` | Format version, Dymini Panel version, schema migration, creation time |
| `master.key` | The 32-byte master key, hex |
| `database.pgdump` | `pg_dump --format=custom --no-owner --no-acl` of the panel database |
| `settings.json` | The secret-free settings export, for humans |

Log lines, metric samples and rollups, and sessions are left out unless
**Include history** is ticked. Restored users sign in again.

The passphrase must be at least 12 characters. Dymini Panel never stores the passphrase
of a downloaded backup. For scheduled jobs it is stored encrypted with the master
key, like every other backup passphrase, so keep a copy somewhere else. Without
the passphrase the backup cannot be opened.

## Taking backups

### Inspecting and verifying scheduled backups

**System → Panel backups** shows the configuration of full-panel and settings-only
jobs: schedule/timezone, destinations, retention, encryption and history inclusion.
Use **Run now** or follow the Backups link to edit jobs and watch active runs.
The destination inventory covers the newest 50 completed managed runs by default
(up to 200), including one row per destination copy. It checks actual object
presence and size; missing and unreachable copies are distinguished. It does not
scan arbitrary bucket objects, import orphaned objects from an older panel
database, or index one-off downloaded backups. Old manifests may be unknown until
verification. Delete removes only the selected destination copy; deleting the
last copy expires its run. Keep a second verified recovery copy.

**Verify backup** starts a serialized, non-destructive background check of that
exact destination copy. The current master key decrypts the job's stored backup
passphrase; that original passphrase, not the master key itself, decrypts the
archive. If the passphrase has changed since the backup, restore the original
passphrase in the job first. Verification downloads into a private scratch
directory, compares the recorded SHA-256, authenticates every encryption frame,
validates the exact regular-file archive entries, master-key encoding, settings
document and format manifest, then runs `pg_restore --list` on the SQL dump.
Settings-only exports get checksum/JSON checks, not SQL validation. The scratch
directory is removed afterward. No SQL is executed and no live data/key is
replaced. This is not a complete restore drill or a guarantee that every SQL
statement or application migration can be restored.

Verification requires compatible PostgreSQL client tools, is limited to ten
minutes, 4 GiB downloaded and 16 GiB expanded (16 MiB for metadata/settings).
The latest result per run/destination is persisted and shown; interrupted checks
can be retried. Automatic weekly verification is not enabled; run Verify through
the UI or MCP on your operational schedule. Restore retains the existing offline
installer/CLI flow below; never restore over the running panel through SQL tools.

### Backup health and alerts

System and Statistics report recorded full-panel successes, age and the worst
failing streak among enabled scheduled full-panel jobs. Settings-only exports do
not count as disaster-recovery protection. Every enabled full-panel schedule is
checked independently: a healthy slow schedule cannot hide a stale daily job.
Freshness expires after two cron occurrences following the last success (or job
creation before the first success), using its timezone and calendar/DST rules.
New jobs have this initial grace period. No enabled full-panel schedule warns
immediately. Health uses recorded outcomes, not remote availability; inspect the
inventory and verify copies to check S3 contents.

The existing 30-second backup scheduler emits `backup.panel_health` warnings when
health changes or once per 24 hours while unchanged, through the existing event
notification channels. Subscribe to that event or `backup.*`. Existing
`backup.failed`/`backup.partial` alerts still report individual run failures;
verification failures emit `backup.panel_verify_failed`. Delivery depends on
configured channels and their event subscriptions.

API/MCP operations use the `backups` scope: `GET /api/v1/system/panel-backups`
(`limit=1..200`), its `/health` endpoint, `POST /{run_id}/verify` with
`{"destination_id":"…"}`, and `GET /{run_id}/copies/{destination_id}/download`
or `DELETE /{run_id}/copies/{destination_id}`. Inventory, downloads and verification
are administrator-only; mutations also require write access. Run-now and job
editing reuse the existing backup-job API. Health also appears on the
monitoring-scoped Statistics overview.

- **System → Panel backup** downloads one now (admin with write access).
- **System → Scheduled panel backups** creates a backup job with target
  `panel_db` (database and master key) or `panel_settings` (settings export only),
  uploaded to S3 destinations on a cron schedule with retention, like any other
  backup job. Runs show up on the Backups page; failures raise `backup.failed`
  events for alerts.
- From the host: `sudo sh install.sh --backup dymini-panel.tar.zst.enc` asks for a
  passphrase (or reads `DYMINI_PANEL_BACKUP_PASSPHRASE`) and runs
  `dymini-panel-server backup --out -` inside the panel container.

`GET /api/v1/system/settings-export` returns the configuration as JSON. Columns
ending in `_enc` or `_hash`, TOTP secrets, and CSRF tokens are removed at any
depth, so the export cannot be used to restore; use it to review or diff
configuration.

## Restoring on a new host

1. Copy the backup to the new server. To fetch it from S3, use the **Download**
   link of the run on the old panel or any S3 client.
2. Run the installer with `--restore`:

   ```bash
   curl -fsSL https://github.com/dymini/panel/releases/latest/download/install.sh -o install.sh
   sudo sh install.sh --restore dymini-panel-20261011-084801.tar.zst.enc
   ```

   It asks for the passphrase (or reads `DYMINI_PANEL_RESTORE_PASSPHRASE`), starts
   PostgreSQL, and runs `dymini-panel-server restore` from the panel image before the
   panel starts. The restore:

   - refuses a backup whose schema is newer than the binary (install that version first);
   - refuses a database that already has tables and a different existing master key, unless `--force` is given;
   - restores in one transaction, so a failure leaves the database untouched. PostgreSQL 17's `pg_restore` sends `transaction_timeout`, which PostgreSQL 16 does not understand; a restore onto 16 skips that setting and loads the script with `psql`;
   - writes `/etc/dymini-panel/master.key` (mode 0600) from the backup.

3. The panel starts, applies newer migrations, and republishes the local Traefik
   configuration. Containers do not exist on the new host yet: deploy each service
   (or restore its data backups) to bring it back. Agents on other servers keep
   their certificates, because the agent CA is restored, and reconnect once DNS or
   `DYMINI_PANEL_AGENT_URL` points at the new panel.

`--force` also replaces a running installation on the same host: it stops the
panel, drops the panel database schema, and restores the backup.

## Restoring by hand

Without the installer:

```bash
DYMINI_PANEL_BACKUP_PASSPHRASE=... dymini-panel backup decrypt --in dymini-panel.tar.zst.enc --out panel.tar.zst
zstd -d panel.tar.zst && tar xf panel.tar
pg_restore --no-owner --no-acl --single-transaction -d "$DATABASE_URL" database.pgdump
install -m 0600 master.key /etc/dymini-panel/master.key
```

Or, with the panel binary and `DATABASE_URL` set:

```bash
DYMINI_PANEL_RESTORE_PASSPHRASE=... dymini-panel-server restore --file dymini-panel.tar.zst.enc --master-key-file /etc/dymini-panel/master.key
```
