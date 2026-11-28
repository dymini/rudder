# Monitoring and alerts

Dymini Panel samples every server and every running service every 10 seconds. The samples are stored in the panel's Postgres database, rolled up for charts, and checked against your alert rules.

## Global statistics

**Observe → Statistics** combines all servers and services for `1h`, `24h`, `7d`,
or `30d`. It refreshes every minute. `GET /api/v1/stats/overview?range=1h` returns
the same snapshot; agents need `monitoring:read` or the explicit
`get_v1_stats_overview` tool. The API defaults to `1h` and rejects other ranges.

- Traffic and resource charts use 1-minute buckets for 1h, 5-minute buckets for
  24h, and hourly buckets for 7d/30d. Rollup watermarks select disjoint resolution
  windows; finer rollups and raw samples fill the unrolled live tail. Proxy
  traffic is available through the latest completed minute, not the access log's
  currently accumulating minute. No raw-log scan is needed.
- Network bytes integrate stored byte rates using observed sample counts and
  the nominal 10-second cadence, capped at each bucket's duration. Proxy counts
  convert the stored requests-per-minute values back to counts. Partial edge
  buckets are proportionally clipped. These are sampled estimates, not billing
  counters; irregular sampling, gaps, and partial buckets can affect accuracy.
  Server network totals never add service traffic on top of host traffic.
- CPU and memory averages weight each server's samples, then average the
  reporting servers equally. Peaks are the highest observed server sample.
  Memory percentages describe servers; service rankings use average bytes.
  Five independent top-10 lists link to currently existing services. Historical
  traffic for deleted resources remains in totals until metric retention removes it.
- Container restarts are **observed positive changes in sampled service restart
  counters**, using bucket peaks for rollups. Initial counter values are not
  counted. Resets, replacement containers, and changes hidden inside a rollup
  can be missed; this is not a complete restart-event audit.
- Deployments are selected by creation time. Success rate uses finished deploys;
  average duration uses finished runs with valid start/end timestamps. Backups
  use start time, exclude restores, and count `partial` as a failure. Each job's
  last fully successful backup is shown across all time, independent of range.
- Alerts count `alert.fired` events, not notification retries. MCP counts finished
  calls; its error numerator excludes denied calls (reported separately). Inventory
  counts are current. Domain count means service-domain rows; panel/Access
  settings hosts are not additional service-domain records. Plugin count means
  installed plugin records, not available catalog entries.
- Disk values show the latest retained sample per current server, independently
  of the selected range, with an hourly fallback and its timestamp (up to 30d).
  An hourly fallback is an average for that bucket, not an instantaneous reading.

The response exposes active metric retention settings. Defaults are raw 24h,
1m 7d, 5m 14d, and 1h 30d. Shorter custom retention, downtime, or expired event
history can make a range incomplete; the dashboard does not invent missing data.
Missing resource metrics are `null`/`—`; absent request traffic is zero. In-flight
MCP calls are excluded. New time-leading indexes support range scans, and backup
last-success lookups use a partial job/time index. The metric and operational
queries share a repeatable-read database snapshot.

## What is measured

| Scope | Metrics |
| --- | --- |
| Server | `cpu_percent`, `memory_percent`, `memory_used_bytes`, `disk_percent`, `disk_used_bytes`, `load1`, `net_rx_bps`, `net_tx_bps` |
| Service | `cpu_percent`, `memory_bytes`, `memory_percent` (when a memory limit is set), `net_rx_bps`, `net_tx_bps`, `restarts`, `containers` |
| Service, from the proxy | `requests` and `http_5xx` per minute, counted from the Traefik access log |

Server metrics come from `/proc` (`stat`, `meminfo`, `loadavg`, `net/dev`) and from `statvfs` on the data directory. When the panel runs in a container, mount the host's `/proc` read-only and set `DYMINI_PANEL_HOST_PROC=/host/proc`, otherwise you see the container's own view. The installer does this for you. Loopback, `veth`, Docker bridge, and CNI interfaces are left out of the network totals.

Service metrics come from the Docker stats API for containers labelled `dymini-panel.managed=true`. A compose stack's containers are added together. CPU is a share of the whole server, like the server's own CPU, so a service that keeps one core of a 4-core server busy shows 25%.

The panel measures its own server. Remote servers send theirs through the agent.

## The overview

The overview page (**Deploy → Overview**) puts what needs a person first:

- A health summary that says whether everything runs normally, and otherwise lists each problem as a row with the button that fixes it: services that failed (**Show them** filters the service list), servers that went offline, alerts that fire, the panel server being busy or out of memory, and paused builds (**Resume builds**).
- Four tiles: servers online, services running, builds running and waiting, and alerts firing. Each opens its page.
- **Heaviest services**: the services that use the most CPU or memory right now, with a **Stop** button for admins. A compose stack is stopped with `docker compose down` (volumes are kept).
- **Panel server load**: memory and CPU of the server the panel runs on, the load guard's level and why, and buttons to pause or cancel builds. See [the load guard](load-guard.md).
- The servers with their CPU, memory and disk, and the recent activity.

Every page has a search box in the header (**Ctrl K**, **⌘K**, or **/**) that jumps to any page or service and runs common actions such as switching the theme. Next to it a dot says how the panel answers: **Live**, **Slow** when a read took longer than 2.5 seconds, **Busy** when the panel asked to retry, or **Reconnecting…** when it did not answer. Pages keep retrying with growing pauses, so an overloaded panel is not flooded by open tabs.

## Charts and resolutions

Open **Monitoring** for the server overview, per-server charts, and a table of services sorted by CPU use. Each service also has a **Metrics** tab.

| Range | Data used |
| --- | --- |
| up to 1 hour | 10-second samples |
| up to 6 hours | 1-minute rollups |
| up to 48 hours | 5-minute rollups |
| longer | hourly rollups |

A job runs every minute. It builds 1-minute rollups (average, minimum, maximum) from the samples, 5-minute rollups from those, and hourly rollups from the 5-minute ones. Each resolution keeps a watermark, so a restart picks up where it stopped and never counts a bucket twice. A Postgres advisory lock keeps two panels from rolling up at once. Request counts are kept as per-minute rates at every resolution.

### Retention

| Data | Default | Range |
| --- | --- | --- |
| 10-second samples | 24 hours | 1–168 hours |
| 1-minute rollups | 7 days | 1–365 days |
| 5-minute rollups | 14 days | 1–365 days |
| Hourly rollups | 30 days | 1–365 days |

Admins change these on the Monitoring page or with `PUT /api/v1/settings/retention` and `{"metrics":{"raw_hours":24,"m1_days":7,"m5_days":14,"h1_days":30}}`. An hourly job deletes expired rows and drops whole daily partitions once they are past every limit.

## Alert rules

| Kind | Target | Fires when |
| --- | --- | --- |
| `server_cpu`, `server_memory`, `server_disk` | server | the 30-second average is above the threshold (%) for the duration |
| `server_offline` | server | a remote server has not checked in for the duration |
| `service_cpu` | service | the 30-second average CPU is above the threshold (%) for the duration |
| `service_memory` | service | memory is above the threshold in MiB for the duration |
| `service_down` | service | the service should be running but no container is up for the duration |
| `http_errors` | service | more than the threshold 5xx responses arrived within the duration (at least 60 seconds) |
| `event` | event patterns | a panel event matches, such as `deploy.failed`, `backup.*`, or `alert.*`, at or above a minimum severity |

A rule moves from `ok` to `pending` when the condition first holds, and to `firing` once it has held for the whole duration. When the condition clears, the rule goes back to `ok` and Dymini Panel sends a "resolved" notice. Each firing and each resolution is stored as an event, which you can see under **Alerts → Recent events** or with `GET /api/v1/events`.

Events from deploys (`deploy.succeeded`, `deploy.failed`), builds (`build.failed`), backups, and alerts can all be forwarded with an `event` rule.

## Notification channels

| Kind | Settings |
| --- | --- |
| Telegram | bot token and chat ID. `api_url` can point at a self-hosted Bot API server. |
| Discord | webhook URL (HTTPS) |
| Slack | incoming webhook URL (HTTPS) |
| Webhook | URL, plus an optional signing secret of at least 8 characters |
| Email (SMTP) | host, port, security (`starttls`, `tls`, or `none`), username and password, from, and a list of recipients |

Only admins manage channels. Secrets are encrypted with the master key, and the list shows only a summary such as "POST to example.com, signed". When you edit a channel, leave a secret field empty to keep the stored value. **Send test** sends a sample notice right away.

Notices are sent every 5 seconds. A notice that fails on one channel is retried on that channel only, up to five attempts in total. Channels that already received it are not sent it again.

### Webhook payload

```json
{
  "id": 42,
  "kind": "alert.fired",
  "severity": "critical",
  "title": "Alert firing: api cpu",
  "message": "93.4% (threshold 80%)",
  "server_id": null,
  "service_id": "5b0c…",
  "created_at": "2026-10-11T06:40:00Z"
}
```

The request carries `X-Dymini-Panel-Event: <kind>`. With a secret, it also carries `X-Dymini-Panel-Signature: sha256=<hex>`, the HMAC-SHA256 of the raw body. Check it like this:

```python
import hmac, hashlib
expected = "sha256=" + hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
assert hmac.compare_digest(expected, request.headers["X-Dymini-Panel-Signature"])
```

## Docker cleanup

**Servers → Prune** removes dangling images and the build cache on that server and reports the space reclaimed. Tick **Daily prune** to do this once a day. Running containers, tagged images, and volumes are never touched.

## API

| Method | Path | Notes |
| --- | --- | --- |
| `GET` | `/api/v1/metrics` | `server_id` or `service_id`, `metrics` (comma list, up to 8), `range` (`15m`, `1h`, `6h`, `24h`, `7d`, `30d`) |
| `GET` | `/api/v1/metrics/overview` | Latest values for every server and service |
| `GET` | `/api/v1/alert-kinds` | The table above |
| `GET`/`POST` | `/api/v1/alerts` | `{"name","kind","target_spec":{"service_id":"…"},"threshold":80,"duration_secs":300,"channel_ids":[]}` |
| `PUT`/`DELETE` | `/api/v1/alerts/{id}` | |
| `GET`/`POST` | `/api/v1/notification-channels` | Admin. `{"name","config":{"kind":"slack","webhook_url":"…"}}` |
| `PUT`/`DELETE` | `/api/v1/notification-channels/{id}` | Admin |
| `POST` | `/api/v1/notification-channels/{id}/test` | Admin |

Webhook and custom Telegram Bot API URLs are server-side egress destinations. They must resolve to public IP addresses by default; loopback, private, link-local, multicast, and other non-public ranges are rejected during delivery. If a trusted internal notification endpoint is required, set `DYMINI_PANEL_NOTIFICATION_ALLOW_PRIVATE=true` explicitly on the panel.
| `POST` | `/api/v1/servers/{id}/prune` | Needs write permission |
| `PATCH` | `/api/v1/servers/{id}` | `{"auto_prune": true}` |
| `GET` | `/api/v1/events` | `limit` up to 1000 |
