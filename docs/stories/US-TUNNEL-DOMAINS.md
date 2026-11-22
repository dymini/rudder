---
contract: null
created_at: "2026-11-22T15:48:37.890040569+00:00"
e2e: 0
evidence: "Originating integration gap traced to 3e5b013: tunnel creation only scaled replicas, Cloudflare sync only ran on manual publish, and local domain changes only wrote Traefik. Added auto sync on creation and active local publishes without recursion. cargo fmt check and SQLX_OFFLINE workspace clippy passed; server lib plus Cloudflare, edge and stack integration tests passed. Fake Cloudflare regression covers initial app/panel domains, new/deleted domains, panel edits, DNS conflicts, retry on sync error, empty ingress and teardown. SQL text unchanged. Real Cloudflare and cloudflared connectivity unverified."
id: US-TUNNEL-DOMAINS
integration: 1
lane: normal
notes: null
platform: 0
status: implemented
title: Automatically reconcile tunnel domains
type: story
unit: 1
updated_at: "2026-11-22T15:52:18.150104772+00:00"
verify: null
---

# Automatically reconcile tunnel domains
