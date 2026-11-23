---
created_at: "2026-11-23T18:50:33.608651306+00:00"
doc: docs/decisions/0027-cloudflare-multi-zone.md
id: "0027"
links:
  - US-126
notes: "Use one connected account and token. Discover all pages before updating visibility, backfill the former zone enabled, and leave new zones disabled. Resolve the most-specific known suffix before checking enablement and accessibility so a disabled child never falls back to its parent. Keep account-level tunnel, Access, SSH and Turnstile configuration separate from DNS zones. Record publication by zone and keep routes and ownership on disabled or invisible zones. Serialize zone changes and DNS mutation using a PostgreSQL advisory lock; report per-host and per-zone failures and retry idempotently. Cleanup uses the recorded zone, with no DNS deletion on discovery loss."
status: accepted
title: Cloudflare zones share one account and preserve DNS on visibility loss
type: decision
updated_at: "2026-11-23T18:50:33.608652158+00:00"
verify: null
---

# Cloudflare zones share one account and preserve DNS on visibility loss
