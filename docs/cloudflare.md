# Cloudflare Zero Trust

An optional, advanced mode in **Security → Cloudflare**. It puts the panel and opted-in app domains behind Cloudflare, so the server can stop listening on ports 80, 443, and 3000 and SSH can go through Cloudflare too. Direct domains still need public ports. Nothing in it is required: an install that never opens the tab behaves exactly as before, and each step can be undone. See [Domains](domains.md) for per-domain routing, proxied DNS without a tunnel, and the global inventory.

The decision record is [0025](decisions/0025-cloudflare-zero-trust.md).

## Rudder Access through the tunnel

Published service and plugin hostnames go to Traefik over HTTPS on the proxy
network, not directly to their containers. Access-protected routes keep the
forward-auth middleware and the `/_rudder` session handoff route. Declared plugin
agent API bypasses still require the application's bearer token. Panel-host
Access login redirects use HTTPS when that hostname is tunnel-published, even
when direct-origin TLS is disabled.

The Access portal must also be reachable. It can be in **any enabled zone in the
connected account**. For example, apps in `dymini.com` can use
`access.knotree.site` after discovering and enabling `knotree.site`. In Access
settings choose an unused hostname, update Turnstile/identity-provider hostname
allowlists and callbacks yourself, then publish the tunnel again. A hostname
outside the enabled, accessible zones is skipped; it still needs working direct
HTTPS ingress until repaired. Existing portal settings are never silently moved.

**Security → Cloudflare → Close the public ports** and
`GET /api/v1/cloudflare/preflight` (also available through MCP) include a required
`rudder_access_portal` diagnostic naming an unpublished portal and how to repair
it. Do not close public ports until it passes. While a tunnel is published,
saving a portal without an enabled, accessible matching zone is rejected with
`portal_outside_tunnel_zone`.

## Zones and upgrade safety

Rudder connects **one account and one API token**, with multiple selected zones.
Tunnel/connectors, Cloudflare Access applications and SSH remain account-level;
Turnstile remains a separately configured widget and hostname allowlist. Rudder's
origin cache, rate limiting and certificates do not become Cloudflare zone policies.

Saving credentials discovers all pages from `GET /zones?account.id=…`. **Refresh
zones** repeats discovery without changing the selected zones. Newly discovered
zones start disabled. A complete successful discovery marks absent zones
inaccessible; a failed discovery marks visibility uncertain without deleting DNS.
On upgrade the former configured zone and its publication history are backfilled
as enabled. Add **Zone Read** to existing tokens before refreshing, and retain
**DNS Edit** for each enabled zone. A legacy `zone_id` in the credentials request
is still accepted and enables that zone; it is no longer required.

For every hostname, Rudder normalizes case and matches the apex or a dot-boundary
suffix, taking the most-specific known zone. A disabled, inaccessible or inactive
child zone blocks the hostname: Rudder never falls back to a parent zone. The
Domains inventory shows the matched zone, diagnostics and a zone filter.

Publication reports outcomes per zone and per hostname. A failing zone does not
prevent other zones from publishing. Existing DNS and tunnel routes are retained
when a zone is disabled or becomes invisible. Retry **Publish again** after fixing
permissions or conflicts; retries reuse matching records. Discovery does not prove
DNS-write access: write failures are reported when publication runs.

Cleanup and teardown use recorded zone ownership, not today's hostname resolution,
and never delete foreign records. Disabled or inaccessible zones are left alone,
with teardown warnings; retained records may need manual cleanup after access is
restored. Disabling a zone is **not** an unpublish or security-deny action.
Changing zone selection is allowed with a running tunnel; changing accounts still
requires removing the tunnel. Legacy status `zone_id` and `zone_name` fields show
the enabled primary zone, or the first enabled zone, or empty strings when none
are enabled. New integrations should use the zones API instead.

## Connector management and rolling updates

The tunnel card shows Cloudflare's connectors (connector ID, cloudflared version,
origin IP, colo and connection time) alongside local containers (name, image,
digest, executable version, state, uptime and restarts). Failed Cloudflare,
Docker, or GitHub checks are shown separately rather than hiding the other data.

**Check for updates** reads stable releases from `cloudflare/cloudflared` on
GitHub. Results, errors and the last successful release survive panel restarts;
automatic checks are cached for six hours. New managed replicas use a saved,
version-pinned `cloudflare/cloudflared:<version>` image, not a floating tag.
Existing containers are not restarted just by opening the page or checking.
An explicit `RUDDER_CLOUDFLARED_IMAGE` still wins; managed rolling updates are
disabled until that override is removed.

**Update connectors** runs in the background, one replica at a time, including
when there is only one replica:

1. Pull the pinned target image and start `<replica>-next` on the proxy network.
2. Wait up to three minutes for that container's registered-connection log and
   a successful `/ready` probe. Merely counting unrelated tunnel connections is
   not enough to retire the original.
3. Rename the still-running original to `<replica>-previous`, give the connected
   replacement its normal name, then gracefully stop the original (up to 60
   seconds to drain) and remove it.

A startup/readiness/cutover failure removes the surge and keeps the original.
If retiring the original fails, Rudder restores it to a healthy state before
removing the replacement. If Docker cannot restore the original, the registered
replacement is deliberately kept to avoid downtime; the update stops and shows
the error. Retrying recovers
interrupted `-next`/`-previous` containers only after proving the survivor is
healthy. Already completed replicas stay updated on a later failure; the saved
target version changes only after the whole rollout succeeds. Interrupted jobs
are visible after restarting the panel. Updates, scaling, credential changes
and removal share a database lock; overlapping mutations return 409.

DNS and zone mutations are also serialized. Completed DNS reconciliation and tunnel publication release their zone lock before the next routing phase begins, so a single domain save does not conflict with its own completed work. A genuinely concurrent mutation still returns 409; wait for it to finish before retrying.

The same controls are available through API/MCP in the existing Cloudflare
scope: `GET /api/v1/cloudflare/tunnel/status`,
`POST /api/v1/cloudflare/tunnel/check`, and
`POST /api/v1/cloudflare/tunnel/update` with `{}` for the latest cached stable
release or `{"version":"2026.11.0"}` for an explicit version (including a
downgrade or retry).

## What you need

- A domain whose DNS is on Cloudflare (the zone).
- A Cloudflare account with Zero Trust enabled (the free plan is enough for a few people).
- An API token with these permissions:
  - Account · Cloudflare Tunnel · Edit
  - Account · Access: Apps and Policies · Edit
  - Account · Access: SSH Auditing · Edit (only for SSH)
  - Account · Cloudflare One Connectors/Networks · Edit (only for SSH)
  - Zone · Zone · Read, for discovery of every zone the token should expose
  - Zone · DNS · Edit, for every enabled zone

The token and the tunnel token are encrypted with the master key and are never returned by the API. Leave the token field blank to keep the saved one.

## Steps

Do them in order. The page shows what each step did and what is still missing.

1. **Cloudflare account.** Save the account id and token, then enable the zones Rudder may manage in the zones table. Zone ID is optional. Refresh after changing token permissions. Rudder checks account API access; per-zone DNS errors are reported during publication.
2. **Tunnel.** Create one tunnel and choose one to five replicas. Each replica is a `rudder-cloudflared-N` container on the `rudder-proxy` network that connects out to Cloudflare, so no inbound port is needed. With two or more, the apps stay online when one replica or one Cloudflare connection is lost. Changing the number adds or removes containers without restarting the others.
3. **Publish domains.** Creating the tunnel automatically sends app domains whose routing mode is **Cloudflare**, the panel domain, and the Rudder Access portal through the tunnel, with proxied CNAME records. Domain changes are synchronized automatically while tunnel mode is active. New service domains default to **Direct**; opt in to Cloudflare in each domain's settings. Upgrading preserves existing service domains on an already-published local tunnel. Rudder does not replace foreign A, AAAA, or CNAME records unless you tick the overwrite box; the page lists names it skipped. Tunnel domains use Cloudflare's certificate without Let's Encrypt or HTTP redirects; direct domains retain their TLS settings and need public ports. Use **Publish again** to retry failures or resolve conflicts. If tunnel creation reports a publication error, the tunnel is still saved; retry publishing instead of creating another tunnel.
4. **Cloudflare Access for the panel.** Enter your team domain (`myteam.cloudflareaccess.com`) and the emails allowed in. Rudder creates an Access application for the panel domain. Rudder's own sign-in, authenticator, and Rudder Access for internal apps stay in place behind it.
   - **Let AI agents and API clients through** adds Access bypass applications for `/mcp`, `/oauth`, `/.well-known`, and `/api/health`, so MCP clients and the `rudder` CLI (`rudder login --browser`, `rudder login --device`, `rudder push`, `rudder deploy`, etc., routed via `/mcp/api/v1/*`) can still reach the panel. They still need their own `rdr_...` tokens (and browser approval on `/tokens` for CLI sign-in).
   - **Require Access** makes the panel refuse requests for its own domain that do not carry a valid `Cf-Access-Jwt-Assertion` (signature, issuer, audience, and expiry are checked against the team's published keys). Turn it on only after you opened the panel through its Cloudflare address and signed in with Access; Rudder refuses to turn it on from a request that does not already carry a valid token, so you cannot lock yourself out. Turning it off is always allowed.
5. **SSH through Cloudflare** (optional). Cloudflare signs a short-lived SSH certificate for each login, so no SSH keys are kept on the server.
   - Enter an IPv4 address the tunnel can reach port 22 on (for example the Docker gateway of the `rudder-proxy` network) and the Unix accounts that may log in.
   - Rudder creates the route, the infrastructure target and application, and reads the account's SSH CA. It does not edit `sshd_config`. Instead it shows a script to run as root on the server; the script makes `sshd` trust the CA and checks the configuration before reloading.
   - After you connected once, press **I ran it and connected**. Until then SSH is not counted as ready.
   - Each person needs the Cloudflare One Client with the Gateway proxy for TCP turned on.
6. **Close the public ports.** The page runs a preflight: tunnel connected, replicas running, domains published, Access enforced, and SSH confirmed when it was set up. When every required check passes it shows the installer command:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/vantanminh/rudder/main/deploy/install.sh | sudo sh -s -- --close-ports
   ```

   `--close-ports` refuses to run unless a tunnel replica is running. It stops publishing ports 80 and 443 on Traefik and publishes the panel on `127.0.0.1:3000` only; port 7443 stays open for agents on remote servers. The choice is remembered in `/etc/rudder/ports.closed`, so upgrading later does not reopen anything. Run the installer with `--open-ports` to undo it.

   Rudder never closes its own listener from inside the panel, and it does not change the firewall. Port 22 belongs to your provider's firewall: close it there once SSH through Cloudflare works.

## Removing it

**Remove the tunnel** deletes replicas, Rudder-owned tunnel CNAME records, Access and SSH applications, the route, and the target. Service domains still set to Cloudflare routing fall back to proxied A/AAAA records when their server has a valid public IP; publication warnings explain any missing address. The panel and Access portal no longer use the tunnel, so point their DNS at the server yourself. Origin TLS returns to each domain's configured settings, including Let's Encrypt. If public ports were closed, run the installer with `--open-ports` first.

## Behind the tunnel

- The visitor's address comes from `CF-Connecting-IP`, trusted only when the tunnel is on and the request arrived from a private or loopback hop. It feeds rate limits, the Access gate, and the audit log.
- The panel's Access check is cached for 30 seconds and Cloudflare's keys for 10 minutes.
- A Cloudflare error is shown as a message on the page (HTTP 424); it never makes the panel look offline.

## For AI agents

Everything above is available through the HTTP API and the MCP server under the `cloudflare` scope. **Full control** can change it; the **Operate** template can only read the status and the preflight. Audit actions start with `cloudflare.`.

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/v1/cloudflare` | Settings, tunnel health, and what each step has done |
| PUT | `/api/v1/cloudflare/credentials` | Save account/token, discover zones; optional legacy `zone_id` enables that zone |
| GET | `/api/v1/cloudflare/zones` | List zones with enablement, visibility, status and errors |
| POST | `/api/v1/cloudflare/zones/refresh` | Refresh the paginated zone inventory without deleting DNS |
| PUT | `/api/v1/cloudflare/zones/{zone_id}` | Set `{"enabled":true}` or `false`; no DNS deletion |
| POST, DELETE | `/api/v1/cloudflare/tunnel` | Create the tunnel, or remove it and everything it made |
| PUT | `/api/v1/cloudflare/tunnel/replicas` | Set the number of replicas (1 to 5) |
| GET | `/api/v1/cloudflare/tunnel/status` | Connections, local containers, versions and update progress |
| POST | `/api/v1/cloudflare/tunnel/check` | Refresh the cached stable cloudflared release |
| POST | `/api/v1/cloudflare/tunnel/update` | Rolling update to the latest release or an explicit `version` |
| POST | `/api/v1/cloudflare/tunnel/publish` | Publish the domains; `overwrite_dns` replaces existing records |
| PUT | `/api/v1/cloudflare/access` | Create or update the Access application |
| PUT | `/api/v1/cloudflare/access/enforcement` | Require or stop requiring the Access token |
| PUT, DELETE | `/api/v1/cloudflare/ssh` | Set up or remove SSH through Cloudflare |
| GET | `/api/v1/cloudflare/ssh/script` | The script that makes `sshd` trust the CA |
| PUT | `/api/v1/cloudflare/ssh/confirmation` | Record that an SSH login worked |
| GET | `/api/v1/cloudflare/preflight` | The checks to pass before closing the ports |

## Settings

| Variable | Default | Meaning |
| --- | --- | --- |
| `RUDDER_CLOUDFLARE_API_URL` | `https://api.cloudflare.com/client/v4` | Cloudflare API address; only honored by development builds, official releases always use Cloudflare |
| `RUDDER_CLOUDFLARED_IMAGE` | No override; stable release pinned in settings | Custom image for replicas; overrides managed versions and disables managed rolling updates |
