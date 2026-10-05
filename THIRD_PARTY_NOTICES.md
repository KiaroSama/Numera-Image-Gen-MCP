# Third-party notices

First-party Numera code is GPL-3.0-only. No reference implementation source copied.
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
