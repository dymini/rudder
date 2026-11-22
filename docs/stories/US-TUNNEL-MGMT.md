---
contract: null
created_at: "2026-11-22T16:34:34.558975183+00:00"
e2e: 0
evidence: null
id: US-TUNNEL-MGMT
integration: 1
lane: high_risk
notes: "Based on domains after merging origin/main in f4ee0cf and retaining both changelog/i18n groups. Kept main IN-036 and recreated domains IN-037; recreated tunnel intake IN-038 after merging that base. Implemented cached connector/container/release status; explicit image-override precedence; persisted version pins and background progress; surge-first updates serialized with credential/scaling/removal changes. Readiness uses own registration log plus live /ready; originals drain before removal. Failure restores original; if restoration is impossible, retain the registered replacement to preserve availability. Handles interrupted names/jobs. Added Cloudflare UI, vi translations, OpenAPI/MCP, guides and Added changelog entries. Verification: fmt; offline workspace/all-targets clippy; sqlx prepare --workspace -- --all-targets with unchanged metadata; 12 node, 88 server-lib, 6 Cloudflare, 1 edge, 2 OpenAPI and 4 stacks tests passed; web i18n/build and 11 parser/validation tests passed. Logs: /home/ubuntu/rudder-tunnel-{prepare,clippy,tests,final-tests,web}.log. Browser rendering and real Docker/Cloudflare rollout remain unverified."
platform: 0
status: implemented
title: Inspect and safely roll Cloudflare tunnel connectors
type: story
unit: 1
updated_at: "2026-11-22T16:59:59.405151140+00:00"
verify: null
---

# Inspect and safely roll Cloudflare tunnel connectors
