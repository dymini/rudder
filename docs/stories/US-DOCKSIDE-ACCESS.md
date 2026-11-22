---
contract: null
created_at: "2026-11-22T18:10:30.903299769+00:00"
e2e: 0
evidence: "Core 147 and server 94 unit tests; full workspace other suites passed; Access fixture corrected and final 3/3 passed; stacks 4/4; backup 3/3; OpenAPI/MCP parity passed. SQLx prepare --all-targets and offline all-target clippy passed. Web i18n 1823/1957, tests 11/11, TypeScript/Vite build passed. Dockside 0.2.2 fmt/clippy/tests passed on b1c6f54."
id: US-DOCKSIDE-ACCESS
integration: 1
lane: high_risk
notes: "Browser and real Docker/S3/Access-provider paths unverified. Restores run in the panel process, not crash-resumable; safety runs are retained for operator recovery. Dockside 0.2.2 must be released separately. Logs: /home/ubuntu/rudder-dockside-*.log and /home/ubuntu/dockside-backup-*.log."
platform: 0
status: implemented
title: Protect plugin dashboards and manage consistent recoverable backups
type: story
unit: 1
updated_at: "2026-11-22T18:38:25.277675291+00:00"
verify: null
---

# Protect plugin dashboards and manage consistent recoverable backups
