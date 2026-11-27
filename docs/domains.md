# Domains

Open **Security → Domains** for the hostname inventory across all services and
servers, plus the panel and Dymini Panel Access portal. Each entry identifies its
service or stack and server, routing, TLS mode, uploaded certificate validity
when known, and a fresh DNS check. Let's Encrypt certificate state is unknown;
configured TLS is not proof a certificate has been issued.

## Routing a service domain

Add or edit a hostname in a service's **Domains** tab:

- **Direct** is the default. Manage DNS yourself and point it at the server's
  public IP. Existing TLS, redirect, and uploaded-certificate settings apply.
- **Cloudflare** requires saved credentials and a hostname inside the configured
  zone. Wildcards are not supported by this managed routing option.
  - A local server with a published tunnel uses a proxied CNAME to the tunnel.
    Ingress and DNS are synchronized automatically, including later changes.
  - Without a tunnel, Dymini Panel creates a **proxied A or AAAA** record pointing to
    the server's configured public IPv4 or IPv6 address. Set that address first.
    The server still needs reachable public ports and appropriate origin TLS.

Tunnels are currently managed on the panel's local server only; remote servers
use proxied DNS. Paths on one hostname must use the same routing mode, and a
Cloudflare hostname cannot point to two different Dymini Panel servers.

Cloudflare routing is opt-in even on tunnel servers. Upgrading preserves existing
local service domains when a tunnel is already published. New domains default
to direct; panel and Access hosts keep their existing automatic tunnel behavior.

**Direct routing requires public ports 80/443. If those ports are closed, the
domain will not be reachable.** The API and UI warn conservatively for local
tunnel servers: the installer remembers port closure on the host, not in the
panel database. Reopen ports with the installer's `--open-ports` before relying
on direct routing.

## Record ownership

Existing DNS records are not replaced unless you explicitly select **Replace
conflicting DNS records** (`overwrite_dns: true` in the API). This permission
also allows replacing records not created by Dymini Panel.

Deleting a domain or switching to direct removes only Dymini-Panel-created records.
Dymini Panel tracks the zone, record ID, hostname, type and last-written target; it
leaves independently modified records alone. Older tunnel records are recognized
by their `Managed by Dymini Panel` Cloudflare comment. Records without ownership
evidence are left alone. Shared hostnames retain DNS until their last Cloudflare
route is removed. Before changing credentials to a different account or zone,
clean up old records; the new credentials may no longer have access to them.

## DNS checks and actions

**Re-check DNS** refreshes the inventory, resolving names from the panel server
with two-second timeouts and bounded concurrency. Direct records are compared
with the server's public IP. Cloudflare checks also verify the configured
proxied origin and Cloudflare's published edge IP ranges. Unavailable evidence
is reported as unknown, not success. DNS propagation and caches can delay checks.

Use **Change routing** and **Delete** on service domains. Panel and Access hosts
are read-only here; change them in their respective settings.

## API and MCP

The existing `edge` scope covers these operations:

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/v1/domains` | Inventory and fresh DNS checks |
| GET | `/api/v1/domains/routing-options?service_id=<uuid>&host=<hostname>` | Availability, reason, zone and direct warning |
| GET, POST | `/api/v1/services/{id}/domains` | List or create service domains |
| PATCH, DELETE | `/api/v1/domains/{id}` | Update or remove a service domain |

Create and update accept `routing_mode: "direct"` or `"cloudflare"`, with optional
`overwrite_dns`. Responses include availability and direct-warning fields in
`routing_options`. Inventory reports effective routing as `direct`,
`cloudflare-proxied`, or `tunnel`. MCP agents use the same documented routes.
