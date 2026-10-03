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

For a verified OmniRoute Antigravity connection, select `PROFILE="omniroute"` and pass
`image_size: "4K"` plus `aspect_ratio: "16:9"` as tool arguments. The generic `openai-images` compact
profile does not accept these gateway-specific fields or an undeclared provider-option allowlist.
`size` is an aspect-ratio lookup in the examined gateway, not a resolution tier: `3840x2160` does
not select 4K. Do not change profiles merely from a model prefix; verify the actual gateway first.

An authorized native4K call through OmniRoute3.8.51 returned JPEG5504x3072 (11779138 bytes), with
original bytes/hash/dimensions verified and no local resizing. This proves one account/model request,
not arbitrary pixel-size support. The first attempt exposed a large-Base64 validation stack overflow;
the validator was corrected and a new submission occurred only after explicit cost authorization.

The examined Antigravity Images generation handler builds text-only contents and the edit route
rejects Antigravity. Enabling EDIT_MODE cannot restore dropped reference data. Image2image/model-based
upscale needs a gateway edit implementation that forwards validated inlineData and resolution, or an
independently configured official Gemini endpoint/credential. Neither was silently substituted.
Sources: [generation mapping](https://github.com/diegosouzapw/OmniRoute/blob/c1e30b7676975feb298b49eff6ff58923c04b89e/open-sse/handlers/imageGeneration.ts#L1043-L1115),
[Google image generation](https://ai.google.dev/gemini-api/docs/image-generation).

Troubleshooting: confirm gateway version/account/project/key/model route. A skipped non-chat model
check is not a failed image-generation test. Unknown billing outcome is inspected, never automatically
retried. Detailed immutable sources: [REFERENCES.md](REFERENCES.md).
