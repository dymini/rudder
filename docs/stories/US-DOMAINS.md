---
contract: null
created_at: "2026-11-22T16:09:16.228391706+00:00"
e2e: 0
evidence: null
id: US-DOMAINS
integration: 1
lane: normal
notes: "Merged origin/main after PRs 45 and 46. Preserved main's mobile-log IN-036 and recreated the domains intake as IN-037 via Harness. Retained both domains Added entries, naming guidance, docs links, Vietnamese translations and responsive log changes. Merge verification: fmt, offline workspace all-target clippy, 146 core tests, 79 server library tests, catalog/Cloudflare/edge/stacks/OpenAPI tests, MCP parity, web i18n/build and 11 parser/validation tests all passed. No SQL statements or schema changed in this merge, so existing all-target prepared metadata was reused. Logs: /home/ubuntu/rudder-domains-merge-{clippy,tests,web}.log. Live Cloudflare remains unverified."
platform: 0
status: implemented
title: Manage domain routing and inspect DNS globally
type: story
unit: 1
updated_at: "2026-11-22T16:40:49.539366140+00:00"
verify: null
---

# Manage domain routing and inspect DNS globally
