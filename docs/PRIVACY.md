# Privacy boundaries

Image inputs go only to the selected configured gateway/provider/backend, never a convenience public
upload host. Original bytes and ordinary prompts are preserved; explicitly requesting target_language or upscale
adds in-image translation or preservation/upscale instructions to the prompt. Upstream terms/retention still apply; stateless
native requests do not promise zero provider retention. Generated image rights are separate from code
license and provider terms; no legal ownership guarantee.

API credentials are connection-scoped; untrusted asset fetches receive none, including redirects.
Public asset destinations block private/link-local/metadata by default; explicit local origin/path
allowances are narrow. TLS checks never disabled. Logs omit keys/auth/cookies/tokens/signed query
values, prompts/Base64 and original paths by default; DEBUG does not relax redaction.

Private receipts persist request fingerprints, owned paths and upstream IDs locally. The bounded
private result journal also retains completed image bytes and asset continuation URLs until publication
and receipt indexing commit. These may be sensitive: keep the entire state directory owner-private,
off shared/network drives and outside sync/public backup roots. Tool receipts never expose journal
payloads or URLs. Output bytes, logs/config/receipts are excluded from Git/npm artifacts. No automatic gallery/sync/browser launch.
Private state should reside on owner-controlled local storage. Provider-required thought signatures
are retained in a separate private state table, bound to connection/account fingerprint/model and not
returned in tool receipts or logs. No conversational signature replay API is exposed; stateless refs
remain the portable default. Keep encrypted DPAPI files/current-user
ACL private; environment values are not encrypted. Portable secret files need private permissions.
Compact config permits an explicitly owner-selected plaintext api_key in the private local file.
No key enters public receipts/catalogs/logs; storage binds an account fingerprint, not plaintext.
Do not put this file inside sync/public backup/package roots. DPAPI remains optional on Windows.

One submission policy is local, not a guarantee about upstream gateway retries or billing. Inspect
outcome_unknown and existing jobs before authorizing another operation. Cancellation is not refund.
