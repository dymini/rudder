---
created_at: "2026-11-22T17:56:07.749266665+00:00"
doc: docs/decisions/0026-panel-mfa.md
id: "0026"
notes: "Panel accounts are separate from Access identities. Use the maintained webauthn-rs 0.5.x line with required user verification and discoverable credentials. The relying-party origin comes only from the saved panel domain or configured public URL, never request Host headers. Store serialized ceremony state encrypted server-side with a five-minute TTL and atomic single-use consumption. Persist credential counters and reject regressions against the latest locked row; synced zero-counter credentials remain supported. Passwordless sign-in requires a verified discoverable passkey. Reuse encrypted Access mail provider settings for account OTP, not Access identity authorization or visitor codes; hash codes with an HMAC keyed by the panel master key, separate enrollment/login/reauth purposes, ten-minute expiry, five attempts and transactional resend throttling. Require password and an enabled factor for all factor removals and for adding a method when one already exists. WebAuthn ceremonies are browser-only; passkey inventory/rename/delete remain auth MCP tools. No existing administrator require-MFA policy exists."
status: accepted
title: Panel MFA uses verified passkeys and purpose-bound email codes
type: decision
updated_at: "2026-11-22T17:56:07.749267221+00:00"
verify: null
---

# Panel MFA uses verified passkeys and purpose-bound email codes
