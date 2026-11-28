# Security policy

## Reporting a vulnerability

**Please do not open public issues for security vulnerabilities.**

Report vulnerabilities through [GitHub private vulnerability reporting](https://github.com/dymini/panel/security/advisories/new). This creates a private advisory that only the maintainers can see, so we can coordinate a fix before any public disclosure.

If you cannot use GitHub's reporting flow, open a [discussion](https://github.com/dymini/panel/discussions) asking for a private contact channel — do not include vulnerability details in it.

You can expect:

- an acknowledgement within **3 business days**,
- an initial assessment within **7 days**, and
- credit in the release notes of the fix (unless you prefer to stay anonymous).

## Scope

Dymini Panel is a self-hosted control panel that manages Docker containers on servers the operator owns. The following are in scope:

- the panel server, the `dymini-panel` CLI, and the server agent,
- the install and update pipeline (signed release metadata, minisign verification),
- authentication and authorization (session cookies, TOTP, API tokens, MCP OAuth),
- the agent enrollment and gRPC channel (mutual TLS),
- backup encryption and secret storage,
- the bundled templates and plugins shipped in this repository.

Out of scope:

- vulnerabilities in software you deploy *through* Dymini Panel — report those to the upstream project,
- issues that require the operator to already have admin access to the panel or root access to the host (the panel intentionally runs with broad host access),
- denial of service against your own installation,
- issues in third-party dependencies without a demonstrated exploit path through Dymini Panel.

## Supported versions

Only the **latest release** receives security fixes. Dymini Panel self-updates; keep automatic updates enabled or update promptly when a security release is announced. Security fixes are published as GitHub release notes and, for significant issues, as GitHub security advisories (CVE-numbered where applicable).

## Hardening notes

- Serve the panel over HTTPS (`--domain` on the installer) and do not expose the plain-HTTP port to the internet.
- The panel container needs the Docker socket and broad host access by design; treat anyone with panel admin access as equivalent to root on the host.
- Prefer scoped API tokens with the minimum scopes needed; see `docs/mcp.md`.
- See `docs/security-scanning.md` for how this repository is scanned and how to interpret the baseline in `security/baseline.json`.
