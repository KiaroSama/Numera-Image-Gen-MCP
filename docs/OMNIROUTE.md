# OmniRoute profile

Owner example: `http://127.0.0.1:20100/v1`, api-prefix, independent declared key. No gateway settings
are changed by Numera. Examined revision4399e60057aaee564ec9e31054a5f6c76cc92a2d, release/v3.8.52.

Generation uses images/generations. Discovery first reads its image specialty surface, then explicitly
supplements paginated unified models filtered type=image because specialty drops pagination metadata.
Unknown/failed discovery does not blacklist explicit model IDs.

Antigravity image route forwards text only. Its aspect_ratio and image_size mappings are distinct;
1K/2K/4K tier validation avoids gateway clamping. References/edits must fail before paid submission.
Vision/underlying Gemini native input support does not repair gateway forwarding.

Codex generation n>1 fans out multiple paid operations, so Numera rejects it. Its public multipart
edit route supports up to8 refs but does not forward masks; generic/custom editing defaults1 ref.
Some built-ins including native openai reject edits. Configure only independently verified custom
routes, never assume brand parity. Gateway JSON may include base64, URL or data-URL outputs; actual
bytes determine format, not requested MIME. Gateway internal retry/fallback remains outside this bridge.

Troubleshooting: confirm gateway version/account/project/key/model route. A skipped non-chat model
check is not a failed image-generation test. Unknown billing outcome is inspected, never automatically
retried. Detailed immutable sources: [REFERENCES.md](REFERENCES.md).
