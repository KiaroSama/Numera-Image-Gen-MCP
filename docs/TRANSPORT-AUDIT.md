# Transport and identity audit

Audited base: `8b650377e60d9ed29c581cfce5b2dda64e936e1e` (2026-10-06).
This change preserves the existing receipt/journal recovery fixes and public MCP tools.
No state migration, model fallback, paid retry, automatic regeneration or image re-encoding is added.

## Confirmed corrections

- NIG-01d: the incremental HTTP SSE reader rejected `response.failed` and
  `response.incomplete` before the adapter could persist their authoritative terminal state or
  retain completed image items. Terminal frames now reach the adapter unchanged; a generic error
  remains uncertain. Tests cross the real HTTP reader and public MCP generation boundary.
- NIG-06: an original asynchronous waiter accepted completed images with another or missing job ID.
  A shared identity validator now checks both the waiter and explicit refresh, including job kind.
- NIG-07: a queued operation bound to one account could reread a rotated secret and submit through
  another. Private per-operation credential snapshots now bind identity and HTTP to the same headers.
  They are held in a WeakMap, not serialized into configuration or receipts. New operations resolve
  the current credential; recovery with an unavailable former account remains blocked.
- NIG-08: the pinned DNS callback returned the single-address shape even when Node requested all
  addresses. Both callback contracts now retain the same validated IP. SSRF checks remain active.
- NIG-09: a malformed sibling in an image response aborted the whole result, discarding valid images.
  Structural item failures now become individual errors, while malformed envelopes and explicit
  refusals remain fatal. Accepted originals retain their exact bytes and receive partial receipts.
- NIG-10: closing a per-request Agent with an unread rejected body waited for the provider deadline.
  Incomplete/rejected requests now destroy their own dispatcher; fully consumed successes close
  gracefully. Redirect and byte-limit tests leave a real response body open deliberately.
- NIG-11: ComfyUI uploaded the first reference before discovering an invalid later reference or mask
  binding. All required targets are now checked before any upload, including target collisions.
- NIG-12: `cx/` did not receive the same Codex reference/fanout policy as `codex/`. Verified aliases
  now share policy only; the exact model ID sent on the wire remains unchanged.

## Verification boundaries

Fourteen independent correct-behavior diagnostics failed on the base and passed on the corrected
source. They used actual loopback HTTP, SQLite and image files, Node22.16.0, bundled Undici6.21.2,
Sharp0.34.1 and syntax-only compilation with explicit valid-input/descriptor seams. They are not a
claim of a local locked-runtime build. Persistent regression suites in this change use the repository
fixtures and locked dependencies; exact-head CI is the supported-runtime verification gate.

The new suites cover public MCP-to-HTTP SSE, both async protocols, environment/file credential rotation,
DNS callback modes, rejected-body disposal, six response formats, Comfy reference uploads and both
OmniRoute/9router aliases. Existing suites remain enabled. Coverage gates are not lowered.
Integration review adds controls for empty/blank terminal SSE IDs, misspelled reference input slots
and credential rotation during cancellation's completed-job confirmation. Nested recovery reuses the
same operation snapshot; a separate operation still reads current credentials. Invalid input slots
reject before any upload, and invalid terminal identity remains uncertain.
The SDK dependency group keeps client and server updates together across runtime/dev classifications.
Formatting failures remain failures and print an actionable diff; no workflow commits generated fixes.

Actual Claude/Codex GUIs, live gateway/model accounts, GPU execution, production proxies and the owner's
private database snapshots were not accessed. A passing fixture is not live-provider entitlement proof.
Do not delete historical snapshots, failed/unknown receipts or originals as part of this correction.

## Sources

- https://nodejs.org/docs/latest-v24.x/api/dns.html#dnslookuphostname-options-callback
- https://undici.nodejs.org/api/Dispatcher
- https://developers.openai.com/api/docs/guides/streaming-responses
- https://github.com/diegosouzapw/OmniRoute/blob/994324f54cbae9adc95c8c004c07b235003a407c/open-sse/config/imageRegistry.ts
- https://github.com/decolua/9router/blob/a99cf57239ff778b61e434c2786009d5ed1c412c/open-sse/providers/registry/codex.js
- https://github.com/github/spec-kit

Existing architecture and protocol references remain in ARCHITECTURE.md and REFERENCES.md.
The owner's local Rules, hooks and Spec Kit artifacts are private and are not copied into this PR.
