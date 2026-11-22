---
contract: null
created_at: "2026-11-22T16:09:16.228391706+00:00"
e2e: 0
evidence: null
id: US-DOMAINS
integration: 1
lane: normal
notes: "Implemented opt-in per-domain Cloudflare routing, legacy active-tunnel backfill, mixed TLS rendering, tracked DNS ownership, scoped overwrite, and global inventory with DNS checks. Direct warnings are conservative for local tunnel servers because installer port closure is not persisted in the panel database. Verified fmt, offline workspace clippy, sqlx prepare with --all-targets, 144 core and 79 server unit tests, Cloudflare/edge/stacks/OpenAPI integration tests, MCP parity, web i18n/build and 7 parser tests. Browser component fixtures verified inventory, direct warning, disabled Cloudflare reason, routing save, DNS recheck, empty/error states at 390px and desktop at 1280px; evidence /home/ubuntu/rudder-domains-evidence. Real Cloudflare and cloudflared connectivity are not verified."
platform: 0
status: implemented
title: Manage domain routing and inspect DNS globally
type: story
unit: 1
updated_at: "2026-11-22T16:31:17.685345015+00:00"
verify: null
---

# Manage domain routing and inspect DNS globally
