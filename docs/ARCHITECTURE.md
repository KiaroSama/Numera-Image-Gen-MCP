# Architecture

Stdio SDK transport -> schema-validated tools -> application services -> pure adapter mapping ->
bounded HTTP -> normalization -> validated atomic outputs/transactional receipts.

`config/` owns schema/precedence/secrets. `adapters/` owns native wire contracts; tools never construct
provider payloads. `services/` orchestrates readiness/discovery/generation/recovery. `jobs/store.ts`
uses Node SQLite WAL/FULL transactions for request-ID uniqueness, admission and owned output index.
`files/` validates signatures/decode/pixels/masks/roots and preserves original bytes. `http/` separates
trusted configured API transport from DNS-pinned, credential-free asset downloads. `logging.ts`
provides one shared redacted JSON logger. No bundled diffusion runtime; ComfyUI owns inference.

## Important trade-offs

SQLite is built into Node24 (release-candidate API), avoiding a native DB dependency. State must be
local shared storage, not an unreliable network drive. Unique intent does not prove global exactly-once
execution across installations or prevent a gateway's internal retry/fallback. Unknown requests do
not replay. Crash receipts may remain in-flight until conservative lease expiry; inspect before new
billing authorization.

Native protocols differ: Gemini inlineData is not Interactions input/steps, Responses orchestration
model is not image tool model, and OpenRouter /images is not chat/completions. No recursive Base64
or assistant-link guessing. SVG/animation unsupported; PNG/JPEG/WebP required.

ComfyUI requires user-selected API graphs/bindings, installed node validation and primary GPU evidence.
No inferred checkpoint/node maps, automatic downloads or global shared-backend interrupt.
