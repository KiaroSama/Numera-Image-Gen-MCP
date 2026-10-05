# Architecture

Stdio SDK transport -> schema-validated tools -> application services -> pure adapter mapping ->
bounded HTTP -> normalization -> validated atomic outputs/transactional receipts.

`config/` owns schema/precedence/secrets. Compact config expands into the same advanced connection
contract; it adds no second transport or generation path. Configured model names are presentation only,
exact IDs remain submission identity. Adjacent config discovery is module-relative, never caller CWD. `adapters/` owns native wire contracts; tools never construct
provider payloads. `services/` orchestrates readiness/discovery/generation/recovery. `jobs/store.ts`
uses Node SQLite WAL/FULL transactions for request-ID uniqueness, admission and owned output index.
`files/` validates signatures/decode/pixels/masks/roots and preserves original bytes. `http/` separates
trusted configured API transport from DNS-pinned, credential-free asset downloads. `logging.ts`
provides one shared redacted JSON logger. No bundled diffusion runtime; ComfyUI owns inference.

Env parsing uses Node util.parseEnv after strict bounded UTF-8/key validation. Each tool reads the
selected file and atomically accepts a validated immutable connection/model snapshot. Invalid saves
keep last-good config; storage/security policy changes require restart. Retained snapshots are capped
at64. Existing handles poll/cancel only with matching persisted account/destination identity and
unambiguous transport settings. Region editing prepares a same-dimension native PNG mask; translation
instructions are appended only when target_language is explicitly requested. No local final resizing.

## Important trade-offs

SQLite is built into Node24 (release-candidate API), avoiding a native DB dependency. State must be
local shared storage, not an unreliable network drive. Unique intent does not prove global exactly-once
execution across installations or prevent a gateway's internal retry/fallback. Unknown requests do
not replay. Renewable writer leases carry generation fences: an expired or replaced writer cannot
update receipts or commit output metadata. Dead prepared intents release queue capacity without
submission. Normal completion and job refresh share the same finalization claim.

Completed image bytes are retained in a bounded SQLite result journal before filesystem publication.
A stable item identifier, planned target and hash reconcile a crash after file publication but before
indexing; output index, receipt append and journal commit use one transaction. Publication is not a
cross-filesystem/SQL atomic transaction. Disk/database failure can still prevent initial retention;
Numera reports that failure and never silently repeats paid work. Schema migration takes a consistent
SQLite snapshot including committed WAL state before transactional changes. `get_job(refresh=true)`
recovers retained bytes without the original credential; pending asset URLs use only validated,
credential-free GETs. Polling an existing upstream handle still requires matching destination/account.
Files are fsynced before non-overwriting publication; POSIX also fsyncs the output directory after
link/unlink. Windows does not provide the same directory-sync contract. This is process-crash
recovery, not a universal guarantee against disk failure or power loss. Migration creates a private
consistent snapshot with a SHA256/schema/date manifest; future or incomplete schemas stop safely.
If the journal's SQLite transaction itself cannot persist received bytes, recovery is not guaranteed.

Native protocols differ: Gemini inlineData is not Interactions input/steps, Responses orchestration
model is not image tool model, and OpenRouter /images is not chat/completions. No recursive Base64
or assistant-link guessing. SVG/animation unsupported; PNG/JPEG/WebP required.

ComfyUI requires user-selected API graphs/bindings, installed node validation and primary GPU evidence.
No inferred checkpoint/node maps, automatic downloads or global shared-backend interrupt.
