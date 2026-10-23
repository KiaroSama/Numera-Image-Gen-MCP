# Immutable parity oracle and maintained validation

The native migration has two different evidence sources. They must never be conflated.

## Immutable oracle

Native build, coverage and extracted SDK jobs keep the exact checkout
`fc7da25c3b470199949b1b4c67127d77afa0c34a`, its original lockfile and installed dependencies.
Oracle generators, the 470-case inventory, assertion mappings and retained input provenance continue
using that checkout without patches. It is a historical comparison fixture, not a supported Node
production distribution. Native end-user packages contain no Node or Zod dependency.

## Maintained source validation

The three offline jobs materialize a separate tree from the same Git archive with
`scripts/native-validation.py prepare`. Only three paths differ:

- `package.json` and `package-lock.json`: Zod is pinned to official stable `4.6.7`, including its exact
  npm integrity. All other dependency entries remain unchanged.
- `scripts/bounded.mjs`: reuse the maintained repository supervisor. It joins the owned child close
  event rather than probing a potentially reused numeric child PID. A denied post-close group kill
  is resolved only when independent inspection proves that group absent; other errors fail closed.

Every original test and application source file remains byte-identical. The source-delta receipt
records original/current hashes and unchanged test hashes; CI retains it per platform. The original
coverage, stdio and clean packed-install assertions run without skips or weaker time/worker limits.
Type/lint/build/format checks remain required. Before a strict `npm audit --omit=dev`, the helper
rechecks source hashes, the installed Zod version and bound-before-element-validation behavior using
three elements against a maximum of two. No oversized resource-exhaustion input is needed.

The historical lockfile's Zod `4.6.5` is affected by
[GHSA-mrhx-c7xc-m3w5](https://github.com/advisories/GHSA-mrhx-c7xc-m3w5), reviewed October 23, 2026.
It is not described as audit-clean. The maintained validation dependency is patched; the immutable
oracle remains historical. Neither `npm audit fix --force`, advisory suppression nor ignored audit
exit codes are used. A failure in maintained validation still blocks all signing jobs.

The original macOS frozen supervisor reported `cleanup_verified=false` after a killed descendant was
already absent. Its exact failed syscall was not recorded. Reusing the existing reviewed supervisor
addresses known stale-PID/sticky-error proof defects; actual macOS success must come from hosted CI,
not this source description. Synthetic preparation tests do not prove platform process cleanup.

## Limits

A passing maintained Node suite is not proof that its dependency bytes equal the immutable oracle.
Native parity continues to require its separate original-source inputs and exact-SHA native runs.
Neither check establishes publisher signatures, public OS trust, a verified Release or owner-host
cutover. Those remain independent release gates.

References: [npm ci](https://docs.npmjs.com/cli/v11/commands/npm-ci),
[npm audit](https://docs.npmjs.com/cli/v11/commands/npm-audit),
[Node 24 child lifetime](https://nodejs.org/download/release/v24.21.0/docs/api/child_process.html),
[Zod 4.6.7](https://github.com/colinhacks/zod/releases/tag/v4.6.7),
[upstream bounded-collection repair](https://github.com/colinhacks/zod/pull/6652).
