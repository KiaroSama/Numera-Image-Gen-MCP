# Third-party notices

First-party Numera code is GPL-3.0-only. No reference implementation source copied.

## Native distribution (migration in progress)

Cargo.toml records direct pinned native dependencies. A ready archive must include the actual
resolved dependency licenses/notices and bundled SQLite/codec notices; this document alone is not a
substitute for those files. No native Release is ready until the lockfile and complete transitive
license inventory are verified in hosted CI. RMCP, Tokio, reqwest, rusqlite, serde/serde_json,
ICU4X, image, Base64, SHA2, chrono, regex, UUID and URL retain their upstream licenses.
The authoritative JSON implementation uses no embedded JavaScript runtime. Removed validation/parser
helpers are not native runtime dependencies; the actual archive license inventory follows the resolved
lockfile, not a historical list.
The published `alloc-stdlib 0.3.0` crate omits its BSD-3-Clause notice. The distribution preserves
[the original upstream notice](docs/licenses/alloc-stdlib-0.3.0/LICENSE) from the archive's exact
source revision. [Supplement provenance](docs/licenses/supplements.json) pins the crate checksum,
source revision and notice hash; an unknown missing notice or mismatched supplement fails packaging.
The published `rmcp 3.5.1` crate also omits its repository-root license. Its
[complete original notice](docs/licenses/rmcp-3.5.1/LICENSE), including Apache-2.0/MIT transition
terms and the documentation attribution statement, is preserved verbatim from the archive's source
revision. The package's declared license and the original notice are recorded separately, not
collapsed into a generic Apache text.
The source-side Google dimension table is documented under CC BY 4.0 attribution in the source and
references. Image/model/asset rights remain separate from the software license.

## Historical Node implementation / hosted source oracle

The pinned source oracle uses these existing dependency packages only in CI. Their notices remain
required while those packages are distributed; they are not user prerequisites for a native binary.
Runtime dependencies retain their licenses/notices in distributed npm dependency packages:

| Dependency                   | Locked version | License/source                                                            |
| ---------------------------- | -------------- | ------------------------------------------------------------------------- |
| @modelcontextprotocol/server | 2.3.1          | Apache-2.0, modelcontextprotocol/typescript-sdk                           |
| zod                          | 4.6.5          | MIT, colinhacks/zod                                                       |
| sharp                        | 0.35.5         | Apache-2.0, lovell/sharp; bundled libvips and codecs retain their notices |
| undici                       | 8.11.2         | MIT, nodejs/undici                                                        |

Build/test dependencies include TypeScript7.0.2 native compiler and the official
@typescript/typescript6 6.0.2 compatibility package exposing TypeScript6.0.3 API (Apache-2.0),
Vitest/coverage-v8 5.0.3 MIT,
MCP client2.3.1 Apache-2.0, ESLint10.12.0 MIT, typescript-eslint8.71.1 MIT, Prettier3.9.9 MIT.
package-lock.json records resolved versions/integrity. Preserve upstream package license files and
bundled native notices when distributing. Code license does not establish rights to generated images,
input photographs, models/checkpoints or the owner's logo; provider/asset terms remain separate.
