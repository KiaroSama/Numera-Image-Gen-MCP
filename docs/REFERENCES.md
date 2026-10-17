# References

## Native migration references, checked 2026-10-11

Rust1.99 stable manifest and Cargo environment reference govern hosted-only compiler/workspace cache
setup. RMCP3.5.1 official tag bc82eb861a5ae2e074183e1214dc2380ca454555 was read for ServerHandler,
legacy negotiation, tool/result/resource models and Transport; its uncapped read path is bounded by
Numera's stdio adapter. SDK Apache-2.0 notices remain required. Native runtime compatibility is not
verified merely because these sources were read.

- https://static.rust-lang.org/dist/channel-rust-stable.toml
- https://doc.rust-lang.org/cargo/reference/environment-variables.html
- https://github.com/modelcontextprotocol/rust-sdk/tree/rmcp-v3.5.1
- https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html and retry/proxy references: explicit
  no protocol retry, no inherited proxy, no API redirect, bounded credential-free assets.
- https://docs.rs/serde_stacker/0.1.14/serde_stacker/ and tagged dependency manifest: bounded deep JSON
  parse plus iterative retained clone/drop; MSRV1.67, MIT/Apache-2.0.
- https://docs.rs/base64/0.23.1/base64/engine/general_purpose/struct.GeneralPurposeConfig.html:
  compatibility decoder permits source-accepted unused trailing bits after strict syntax/size checks.
- https://github.com/taiki-e/cargo-llvm-cov/tree/v0.9.1: hosted nightly branch coverage; production
  compiler remains stable, region and branch floors are independently enforced.
- https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex:
  stable Win32 128-bit file identity rather than nightly-only std metadata APIs.
- https://modelcontextprotocol.io/docs/develop/build-server: host command/args launch local stdio;
  stdout must contain only protocol, and local/cloud file access is not interchangeable.

## Historical protocol and Node implementation references

Checked2026-10-03. Interfaces and test patterns only; no third-party implementation copied.
Resume: SDKv2.3.0 release/protocol-versions documentation read; same direct stdio API and Apache-2.0
package licensing. Google thinking/signature and OpenRouter endpoint descriptor docs refreshed.
Current SDK/native source revisions and exact test evidence remain distinct from live availability.

| Source                                                 | Examined revision/license                              | Applied/rejected                                                              |
| ------------------------------------------------------ | ------------------------------------------------------ | ----------------------------------------------------------------------------- |
| https://github.com/modelcontextprotocol/typescript-sdk | e16d27729ea2fae144a6d957fd0372d9eb0e6c58; npm2.2.0 MIT | stable v2 split packages, Standard Schema/stdout/client tests                 |
| https://github.com/diegosouzapw/OmniRoute              | 4399e60057aaee564ec9e31054a5f6c76cc92a2d MIT           | distinct catalog/forwarding, Antigravity refs blocked                         |
| https://github.com/decolua/9router                     | a99cf57239ff778b61e434c2786009d5ed1c412c MIT           | image/info catalog, single-ref contract, terminal done; executor dropped refs |
| https://github.com/Comfy-Org/ComfyUI                   | 1b883beab11c04a2eb82cf6a3ee64294f3303897 GPL3          | API graph/upload/history/view/targeted cancel; no model/node copying          |
| https://github.com/googleapis/js-genai                 | ae573cc7f2d1d4787ac95a3fd8704afbd96c516c Apache2       | native distinct Interactions typed image/model_output steps                   |
| https://github.com/openai/openai-node                  | e39bd99f338d929c3de5a8c45a26f62e653c9523 Apache2       | Images versus Responses; reject SDK default paid retries                      |
| https://github.com/shinpr/mcp-image                    | 19e371f13bc0df25c9166aea2449f0acb65db9d7 MIT           | inspected requestBoundaries tests; reject prompt enhancement/fixed IDs        |
| https://github.com/artokun/comfyui-mcp                 | 6ad6fc0296398c41feff068369a421b94cbb285d MIT           | enqueue/upload tests and CI inspected; preserve subfolders/node_errors        |
| https://github.com/can1357/oh-my-pi                    | 47775c7a3b317aba0cb89a714342af5e89c61b71               | inspected OpenRouter wire usage, no code reuse; official resolution wins      |

Official current documentation read:

- https://openrouter.ai/docs/guides/overview/multimodal/image-generation
- https://ai.google.dev/gemini-api/docs/image-generation and /interactions
- https://ai.google.dev/api/models#method:-models.list
- https://developers.openai.com/api/docs/guides/image-generation
- https://code.claude.com/docs/en/mcp
- https://developers.openai.com/codex/mcp.md
- https://github.com/nodejs/node/blob/v24.21.0/doc/api/sqlite.md
- https://docs.comfy.org/development/comfyui-server/comms_routes

Focused GitHub issues/code/reference searches informed exact route choices; negative search results
never establish absence. OmniRoute source confirms text-only Antigravity and Codex count fanout;
9router issue2797 corroborated in executor source. Gateway-shaped fixtures are not deployed gateway
integration or live entitlement tests. Node docs autonomous fetch robots blocked, official source used.
Canonical GPL3 text fetched from pinned ComfyUI LICENSE after gnu.org connection timeout; complete text
unchanged. Popularity/coverage/benchmarks from references are not Numera facts.

Anthropic-compatible research2026-10-03: officialMessages/vision and SDKoutput ContentBlock union
https://github.com/anthropics/anthropic-sdk-typescript/blob/d49bdab458000bcdffe77bd84b03293f31824fb3/src/resources/messages/messages.ts distinguishimageinput fromoutput. OmniRoute3.8.51 buildc1e30b7/pinnedc1e30b7676975feb298b49eff6ff58923c04b89e
Messages route->chat guard and responseTranslator.ts#L736 dropimagearrays; no claimedMessagesimageadapter.
No upstreamcodecopied, no sharedgatewaymutation or additionalpaidrequest.

Terminal lifecycle sources checked2026-10-05: OpenAI ResponseStatus at
https://github.com/openai/openai-node/blob/fb6955621e1cf6653659adb75094ebace83ebfe9/src/resources/responses/responses.ts ;
Google InteractionStatusUpdateStatus at
https://github.com/googleapis/js-genai/blob/f6b85db43db2cb88705f60af9c394fee308ac497/src/gaos/models/interactions/interaction-status-update.ts ;
Comfy targeted cancellation dispatch/terminal job status at
https://github.com/Comfy-Org/ComfyUI/blob/5c460d8172fe30761ff67c0df3d5643bb74e0d70/server.py and openapi.yaml.
A true cancellation boolean confirms dispatch only, not completion/refund. SDK2.3.1 patch release
https://github.com/modelcontextprotocol/typescript-sdk/releases/tag/v2.3.1 retains stdio contracts.
No third-party implementation copied.
