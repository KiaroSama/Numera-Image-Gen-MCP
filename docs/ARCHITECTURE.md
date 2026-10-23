# Architecture

Native Rust CLI `bridge` -> bounded stdio RMCP transport -> schema-validated ten tools -> application
services -> pure adapter mapping -> bounded HTTP -> normalization -> validated original outputs and
transactional receipts. The tracked production runtime is Rust. All 470 frozen cases and 50 suites have reviewed mappings
and retirement-tree hosted execution at `b513884` / run `39519759155`; signed distribution remains
separate, and later signing-preparation changes require their own hosted verification.

## Boundaries

- `native/config/`: full/compact/dotenv config, defaults/precedence, private credentials/current-user
  Windows DPAPI and immutable last-good reload snapshots. Adjacent discovery is executable-relative,
  never caller CWD. Display names never replace exact model IDs. API credentials are pinned privately
  for the operation and not serialized into receipts/logs.
  `runtime_environment.rs` supplies an instance-owned shared selected-key source; immutable startup
  maps remain supported. A selected value is cloned under a short read lock before credential I/O.
  Accepted config generations share that source, while already-pinned headers remain independent.
  This is not process-global OS environment mutation or Windows environment-key alias emulation;
  [credential snapshot boundaries](testing/CREDENTIAL-SNAPSHOTS.md) retain the acceptance limits.
- `native/adapters/`: Images, Gemini, Interactions, Responses, dedicated OpenRouter, opt-in chat images
  and explicit ComfyUI workflow contracts. Tools construct no provider payloads. Gateway forwarding,
  native model capability and account entitlement are separate evidence layers.
- `native/services/`: discovery, readiness, generation, admission, finalization and existing-handle
  recovery. `generation.rs` coordinates validated inputs; `submission.rs` owns admission, submission,
  existing-job polling and continuation retention. Intent is persisted before paid submission;
  unknown outcomes are never resubmitted.
  Cancellation of local waiting does not establish upstream cancellation or a refund.
  `polling.rs` supplies the production Comfy loop's immediate-first-poll/fixed-500ms/pre-cancel
  scheduling; callers retain receipt/fence/retention and sanitized cancellation-error policy.
  Intent diagnostics expose numeric preparation/admission/reservation/update durations only, without
  changing deadlines or adding event bodies. The cumulative pre-submit duration includes preparation;
  logging's own synchronous write cost is outside those measurements.
- `native/jobs/`: bundled SQLite, preserved schema v2/WAL/FULL, renewable 30-second leases and
  generation fences. Journal phases waiting/retained/publishing/committed keep stable output identity
  across filesystem/SQL crash gaps. State remains local shared storage, not an unreliable network drive.
  An instance-owned persistence port delegates publication write/sync/link/unlink to the original OS
  operations by default. Its receipt-update entry seam supports scoped failure verification without
  changing real SQL error sanitization, generation fences or the database schema.
- `native/files/`, `native/http/`, `native/mcp/`, `native/cli/`: byte/container/path/mask validation,
  API transport separate from DNS-pinned credential-free assets, SDK protocol lifecycle and optional
  explicit host registration. Originals are never resized; derivative previews are separate.
  Native readiness owns a child of the selected MCP executable in `bridge` mode, using an official
  RMCP client and bounded private JSON-line pipes. It lists tools and calls read-only health,
  connections and optional discovery through the actual protocol, then closes/reaps the child and
  emits structured cleanup evidence. Scoped diagnostic draining cannot detach on cancellation; the
  anchored child owner has bounded Drop cleanup. Protocol failures retain JSON reports and exit2.
  POSIX direct bridges inherit outer process-group containment; Windows uses private kill-on-close
  Jobs. Force-killing an unsupervised POSIX parent cannot run destructors. Developer Diagnose uses
  the selected installed executable. These paths passed all seven native contexts at `49dde41`.
  Asset hostname resolution has a per-call seam; default Tokio lookup, all-address validation,
  first-address pinning and redirect checks remain shared. Submission transport observation reports
  numeric client-build/pre-send durations only; preparation is not proof of a network write.

`native/runtime_paths.rs` confines managed temp/cache to the installation root. Logging uses private
UTF-8 UTC structured files and stderr, never protocol stdout; credentials/prompts/private paths stay
redacted at every level. The executable needs no Node/Python/Rust installation. ComfyUI, if selected,
owns inference and GPU/model/node installation; Numera does not bundle or silently install them.

## Direct local HTTP and native menu

The optional `http` command serves the same `Application` and ten-tool `Handler` through official
RMCP Streamable HTTP. Hyper owns bounded connections; a tracked task set and explicit shutdown close
owned work. SDK session handlers have a separate clone-lifetime barrier so connection drain alone
cannot be mistaken for release of application files and database handles. The default endpoint is `127.0.0.1:1969/mcp`, with validated port overrides and no public
binding or silent collision fallback. Host/Origin and separate local Bearer authentication guard
requests before dispatch. SDK-negotiated modern/legacy session and cancellation semantics stay distinct.

`native/cli/http_settings.rs` owns private persistent port/token files independently of image provider
configuration. `native/cli/menu.rs` supplies native start/port/guidance/exit interaction; explicit
`bridge` never displays a menu. No OS service, OAuth relay or reference-project account features are
added. HTTP/menu SDK, CLI and extracted-archive paths passed on all six targets at `49dde41`.

## Native developer tooling and signing

`native/devtools/` owns the separate optional `numera-dev` executable, bounded OS process ownership,
private capture/ledger and prepared-artifact maintenance. Production MCP does not invoke it. Setup
reuses the existing readiness and selected-entry registration boundaries, with explicit paths and
all-five dry-runs before state/install mutations. See [developer tooling](DEVELOPER-TOOLS.md).

The artifact verifier pins the approved public key inside the binary, checks current identity
validity and exact approved logo/member hashes, then hands authenticated bytes to staged replacement
and rollback. This is distinct from native Authenticode/Mach-O verification. Hosted signing helpers
must verify those native formats and the detached manifest before final archive/extracted execution.
Missing private custody or verifier capability blocks a finished distribution, not source checking.
[Signing](SIGNING.md) records self-signed trust limits and pending protected-hosted prerequisites.
No signing secret belongs to a normal build/test job or a downloadable artifact.

## JSON compatibility boundary

Native application values use binary64 numbers and exact UTF-16 strings/keys rather than assuming
`serde_json::Value` can represent every accepted source input. Declared schema fields are validated
directly; unconstrained records keep their original values. Nonfinite values become `null` only at
JSON serialization, and lone surrogate units are escaped rather than replaced. SDK protocol metadata
and bundled schema metadata remain separate from application data. Typed in-process request extensions
and stdio/HTTP boundary codecs carry exact tool arguments and restore IDs/structured results.

URL and SQLite plain-TEXT boundaries retain the source runtime's documented USVString/UTF-8 coercion;
JSON receipt text preserves escaped code units. No database schema change, payload sentinel or hidden
runtime is introduced. Its frozen assertions are included in the 470-case acceptance at `49dde41`;
final release signatures and owner-host cutover are separate verification gates.

## Decisions and recovery

Same request ID plus identical effective inputs/account returns the same durable receipt; a changed
identity conflicts. Canonical hashing preserves source JavaScript number/key/English-collation behavior
for existing records; actual oracle evidence is required before accepting compatibility. Existing
handles use retained matching connection snapshots; ambiguous destinations refuse network recovery.

Completed inline bytes are retained before observing cancellation or publishing files. Existing file
publication is reconciled by output UUID/target/hash, never overwritten. Local completed bytes can be
recovered even when a connection has been removed. Invalid saves retain last-good config; changes to
storage/security/global policy or the 64-snapshot bound require restart.

Hosted CI resolves the real Cargo lockfile, verifies stable native binaries per OS/CPU and packages
allowlisted archives. A separate nightly job measures executable regions >=85% and branches >=80%;
nightly is not the production compiler. The pinned Node source is used only in hosted migration oracle
checks, not as a runtime dependency of the shipped application.

Keep old local runtime/dependencies until the owner switches host config and the native connection is
verified. Migration snapshots, original images, receipts, private configuration and Git history survive
cleanup. [Migration acceptance](testing/MIGRATION-MATRIX.md) lists gaps; source existence is not evidence
that any runtime or coverage gate passed.
