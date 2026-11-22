---
contract: null
created_at: "2026-11-22T18:39:45.134342803+00:00"
e2e: 0
evidence: "SQLx prepare --workspace -- --all-targets, fmt, offline workspace/all-target clippy passed. Final server library 97/97 and panel integration 4/4 passed (Docker S3 case gated); core 147/147, node 12/12, Stats 3/3 and OpenAPI 2/2 passed in workspace run. Initial two failing targets corrected: daily preset fixture used 03:00 rather than midnight; host PG14 clients replaced by compatible PG16. Web i18n 1846 used/1986 translated, scripts 11/11, TS/Vite build passed. Logs: /home/ubuntu/rudder-panel-backups-*.log. UI and external S3/notification delivery visually/live unverified. Managed-only inventory up to 200 runs; latest verification per copy; no automatic weekly verifier; pg_restore --list, not a SQL restore drill."
id: US-PANEL-BACKUP-MGMT
integration: 1
lane: high_risk
notes: null
platform: 0
status: implemented
title: Manage and verify panel disaster-recovery backups
type: story
unit: 1
updated_at: "2026-11-22T18:58:05.583586272+00:00"
verify: null
---

# Manage and verify panel disaster-recovery backups
