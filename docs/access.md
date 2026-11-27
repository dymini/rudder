# Dymini Panel Access

Dymini Panel Access decides who may open an internal app that Dymini Panel is hosting. It is separate from the account that signs in to the Dymini Panel.

The panel account is the operator. Dymini Panel Access people are the coworkers who use the internal apps. An email that has not been added is blocked.

## Panel account multi-factor authentication

**Account → Confirm security changes** holds the current password and factor used
for security changes. Register your first method using your password. When a method
is already enabled, adding another one requires that password and an enabled factor.
These settings protect the panel account, not an Access identity or app session.

- **Authenticator:** existing TOTP and one-use recovery codes still work. Recovery
  codes belong to the authenticator enrollment; disabling it removes those codes.
- **Passkeys:** register several named keys in Account. Rename or remove individual
  keys there. Registration requests a discoverable credential and requires user
  verification (device PIN, biometric, or equivalent). At password sign-in choose
  **Verify with passkey**, or choose **Sign in with passkey** to discover your
  account without entering a password. Keep an independent method or spare key in
  case a device is lost. A non-discoverable authenticator cannot provide usernameless
  sign-in; resident-key support is required during browser enrollment.
- **Email:** configure an email provider and From address in **Dymini Panel Access**
  first (SMTP, Resend, or Cloudflare). Access itself need not be enabled and the
  panel account need not be an Access identity. Opt in from Account and confirm a
  code sent to your account email. Request a fresh code at sign-in, or select email
  in Confirm security changes for re-authentication. Codes are six digits, expire
  after ten minutes, allow five total attempts, and can succeed only once.
  Enrollment, sign-in, and security-change codes are separate. Resends invalidate
  the previous code and are limited to one delivery per account per minute,
  including failed sends. MFA requests also have a ten-per-minute client-IP budget.

At sign-in, any enabled method can satisfy the second factor. No global
administrator require-MFA policy existed before these methods; enrollment remains
per account. Every method removal, including the last passkey or last MFA method,
requires the password plus a current enabled factor. Passkey verification proofs
are valid for five minutes and for one security action or password sign-in only.

WebAuthn uses `DYMINI_PANEL_PUBLIC_URL` when set, otherwise the saved panel domain (HTTPS
unless SSL is disabled). It never derives its RP ID or allowed origin from
request Host/forwarding headers. HTTPS is required except for `http://localhost`.
Use the configured origin in your browser. When HTTPS terminates at a tunnel or
external proxy while panel SSL is disabled, set `DYMINI_PANEL_PUBLIC_URL` to the external
HTTPS URL. Moving the panel to another hostname
changes the RP ID: keep TOTP or email available and register new keys at the new
origin before retiring old ones.

Ceremony state is encrypted in the database, expires after five minutes, and is
atomically consumed before verification. Public-key credentials and counters are
persisted; successful assertions update the latest locked counter. Counter
regressions are rejected, while synchronized passkeys with zero counters work.
Email codes are stored only as purpose-bound keyed hashes; no codes or private
authenticator keys are logged or returned by account-listing APIs.

`GET /api/v1/me/mfa`, passkey listing/renaming/deletion, and email enrollment/removal
are available through the `auth` API and MCP scopes. Browser-only WebAuthn
registration, verification, and passkey sign-in ceremonies are documented in
OpenAPI but excluded from MCP because they require an origin-bound authenticator
interaction. Agent/API credentials do not bypass the password and current-factor
checks on security changes.

## What a person sees

The portal is at `/access` on the panel, for example `https://panel.example.com/access`. You can also give it its own hostname under **Dymini Panel Access**. Point that hostname's DNS at the server. Dymini Panel publishes the route and serves the same portal there. The path stays `/access`.

Sign-in offers two methods:

- **Google.** One redirect URI, shown on the Dymini Panel Access page. Put that exact URI in the Google OAuth client. Dymini Panel accepts the Google account only when Google says the email is verified and the email is already on the allow list. Starting Google sign-in sets a short-lived `dymini_panel_access_google` cookie, and the callback only finishes in the browser that holds it, so a sign-in link opened in another browser is refused.
- **Email code.** Dymini Panel emails a 6-digit code. The code expires in 10 minutes, and five wrong attempts use it up.

Both methods require a Cloudflare Turnstile token. Dymini Panel checks the token on the server with Cloudflare siteverify. The check fails closed: the token must succeed, the action must be `access_login`, and the hostname Cloudflare returns must be one of the hostnames you saved. Do not put `localhost` in that list on a production panel.

After sign-in, the portal lists the internal apps that person may open. Choosing an app sends them to that app's domain. If they are not signed in there yet, Dymini Panel sends them back through the portal and then sets a cookie on the app's own hostname.

The list includes every hostname of each authorized service, including plugins and gated preview domains. Multiple path routes on one hostname are shown once. The legacy API `host` field remains the first hostname; `hosts` contains the complete list. Department descendants and exact-rank grants use the gate's authorization rules. A stopped service remains authorized and listed; a disabled Access policy is not an Access app. Services without a domain are listed with no link.

If Cloudflare Access protects the panel hostname, using that same hostname for the Dymini Panel Access portal also puts `/access`, login APIs, Google callbacks and their assets behind Cloudflare sign-in. Ordinary Dymini Panel Access users may not be authorized there. Access settings and tunnel readiness warn about this combination, including when the portal hostname is left blank. Prefer a dedicated portal hostname, publish its DNS/tunnel route, add it to the Turnstile allowlist, and update Google's redirect URI. Dymini Panel does not automatically create Cloudflare bypass applications or expose any administrative APIs.

## Who may open an app

On the app's **Domains & SSL** tab, turn on **Require Dymini Panel Access** and list the email addresses. You can also allow a whole department or a rank. A department grant includes the departments nested under it. A rank grant matches that rank exactly.

Save, then add a domain if the app does not have one. Dymini Panel stands in front of every request to that domain:

1. Traefik asks the panel's gate.
2. If there is no valid session cookie for that hostname, the browser is sent to the portal.
3. After sign-in, the browser returns to `/_dymini_panel/callback` on the app hostname. Dymini Panel sets an HttpOnly cookie for that hostname only and continues to the original path.
4. Later requests include the cookie. The gate checks that the person is still allowed and passes identity headers to the app:

- `X-Dymini-Panel-Access-Email`
- `X-Dymini-Panel-Access-User`
- `X-Dymini-Panel-Access-Departments` (comma-separated slugs)
- `X-Dymini-Panel-Access-Ranks` (comma-separated slugs)

The callback router takes priority over the app router, bypasses forward-auth only for the reserved `/_dymini_panel` path, and uses the tunnel's HTTPS entrypoint. The callback exchanges a short-lived, single-use code bound to the actual request hostname, checks that the person can still access the service, and sets a Secure, HttpOnly, SameSite=Lax cookie without a Domain attribute. Wrong-host requests do not consume a valid code. Invalid, expired and replayed codes are rejected; unknown `/_dymini_panel/*` paths return 404 rather than loading the panel SPA. `/api/v1/access/callback` remains available as a compatibility alias with the same host checks.

Those headers are trustworthy when network isolation is on, because the app is not published on a host port and visitors cannot connect to it except through Traefik. If you turn isolation off, do not trust the headers. Use the introspect API instead.

The panel hostname and the Access portal hostname are not gated by Dymini Panel Access. Optional Cloudflare Access can still protect the panel hostname; use a dedicated portal hostname as described above when coworkers should not need Cloudflare sign-in.

## Departments and ranks

Create them on **Dymini Panel Access**. A department can have a parent. A rank has a level from 0 to 1000 so an app can order them.

Put a person in departments from the API (`PUT /api/v1/access/identities/{id}/memberships`). Each membership is one department and an optional rank.

The first person you mark as an Access administrator becomes the Access owner. Other administrators, including other panel administrators, cannot remove that owner or take away their administrator flag. The original Dymini Panel owner can.

## Email providers

Choose one under **Dymini Panel Access**:

| Provider | What to save |
| --- | --- |
| Resend | API key and a From address |
| SMTP | Host, and optionally port, security (`starttls`, `tls`, or `none`), username, and password |
| Cloudflare Email Sending | Account id, API token, and a From address on a domain that Cloudflare is allowed to send for |

The From address is either `login@example.com` or `Dymini Panel Access <login@example.com>`. Secrets are encrypted at rest and are not returned by the API. Leave a secret blank when saving to keep the one already stored.

**Send a test email** delivers a short note to the panel account you are signed in as. It does not create a sign-in code.

## Connecting an app's backend

Rotate the service token on the app's **Domains & SSL** tab. Dymini Panel shows it once and, on the next deploy, injects these environment variables unless you already set the same name yourself:

| Variable | Meaning |
| --- | --- |
| `DYMINI_PANEL_ACCESS_TOKEN` | Secret the app uses to call introspect. Prefix `rdr_acc_`. |
| `DYMINI_PANEL_ACCESS_URL` | Panel public URL, when `DYMINI_PANEL_PUBLIC_URL` or the panel domain is set. |
| `DYMINI_PANEL_ACCESS_SERVICE` | Service id. |

The app reads the visitor's `dymini_panel_access` cookie and asks Dymini Panel:

```http
POST /api/v1/access/introspect
Authorization: Bearer rdr_acc_…
Content-Type: application/json

{ "session": "<cookie value>" }
```

A successful response:

```json
{
  "email": "ada@example.com",
  "display_name": "Ada",
  "departments": [
    { "slug": "accounting", "name": "Accounting", "rank": "lead", "level": 40 }
  ],
  "expires_at": "2026-10-24T12:00:00Z"
}
```

Dymini Panel accepts the session only when it was issued for a domain of that same service and the person is still allowed. Map `rank` and `slug` onto the app's own roles.

## Panel API

Panel administrators and agents with the `access` scope use:

| Method | Path | Purpose |
| --- | --- | --- |
| GET, PUT | `/api/v1/access/settings` | Turnstile, Google, email, portal hostname |
| POST | `/api/v1/access/settings/test-email` | Send a test message |
| GET, POST | `/api/v1/access/departments` | List or create departments |
| PATCH, DELETE | `/api/v1/access/departments/{id}` | Update or delete one |
| GET, POST | `/api/v1/access/ranks` | List or create ranks |
| PATCH, DELETE | `/api/v1/access/ranks/{id}` | Update or delete one |
| GET, POST | `/api/v1/access/identities` | List or allow a person |
| PATCH, DELETE | `/api/v1/access/identities/{id}` | Update or remove a person |
| GET, PUT | `/api/v1/access/identities/{id}/memberships` | Replace department memberships |
| GET, PUT | `/api/v1/access/services/{id}` | Read or set the app's allow rules |
| POST | `/api/v1/access/services/{id}/token` | Rotate the service token |

A rule is `{ "kind": "email", "email": "ada@example.com" }`, `{ "kind": "department", "department_id": "…" }`, or `{ "kind": "rank", "rank_id": "…" }`.

The sign-in, logout, portal, callback, and gate routes are for browsers and Traefik. Agents cannot call them. Introspect is for the app's service token, not for an agent token.

## Network isolation

Isolation is on unless the app turns it off. Dymini Panel does not publish the app's ports on the server, so a second app can also listen on port 80. Each container has its own address on the `dymini-panel-proxy` network. When you add a domain, Traefik routes that hostname to the container.

For a Compose stack, published `ports` become container-only `expose` entries on the next deploy. Turn isolation off on that app when something outside Docker must connect to a host port. Databases keep their optional public port.

`PATCH /api/v1/services/{id}/network` with `{ "isolation": false }` stores the choice. Deploy again to apply it.

## Panel owner and authenticator

The first account created at setup is the panel owner. Setup needs the one-time token the panel writes to `setup-token` in its data directory while it has no users (or `DYMINI_PANEL_SETUP_TOKEN`, when set); `install.sh` prints a setup link that includes it, and the file is removed once the owner exists. Only one account can be the owner. Other administrators can do everything else, but they cannot delete the owner or change the owner's role.

The authenticator on **Account** is the second factor for the panel account. When you enable it, Dymini Panel shows eight recovery codes shaped like `abcde-fghij`. Each code works once at sign-in, in the same field as the authenticator code. Store them somewhere safe. They are not shown again.
