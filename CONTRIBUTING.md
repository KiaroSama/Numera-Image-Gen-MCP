# Contributing

Use Node24, npm ci, strict ESM TypeScript and the committed lockfile. Keep source files below800 lines,
modules responsibility-based, adapters pure and tools provider-agnostic. Update related fixture/docs
with behavioral changes. Never add paid POST retries, provider fallback, prompt rewrites, public input
upload or unbounded queues/polling. Unknown capability/outcome remains unknown.

Run npm run check, format:check, coverage and package:check using documented bounded runners.
Routine full checks run hosted CI; use one narrow seam while editing. No real keys/files in fixtures.
Keep original bytes and owned outputs validated; confirm exact HTTP contract, not a mocked success.
GPL-3.0-only first-party changes, dependency/reference licenses retained. No release/npm publication
without owner authorization. Local setup/registration must preserve unrelated host settings.
