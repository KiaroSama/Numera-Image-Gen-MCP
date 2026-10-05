# Compatibility evidence

Protocol compatibility is not a measured percentage of all models. Model names do not prove support.
Every live connection/model/account/size/input mode must be verified separately. The published baseline
and resumed acceptance additions have separate offline evidence. Compact config covers generic Images, explicit OmniRoute/9router and Responses with a separate
orchestration_model; other native protocols use advanced config. Linux/macOS do not require
DPAPI. Hosted matrix now includes macOS with specific POSIX boundary assertions.
Exact versions, coverage and remaining live limitations belong in TEST-RESULTS.md.
The recovery/model-limit changes added on2026-10-05 passed a fresh hosted three-platform matrix
on320f903, including original-byte normal/recovered comparison, real child interruption and packed
installation fixtures. See TEST-RESULTS.md for exact counts and coverage; this is not live-provider proof.
Canonical Pro/preview dimension evidence describes the public model, not Antigravity account access;
the separately repaired gateway still has no verified successful Pro generation.

| Profile                | Generation                           | Reference/edit route                        | Evidence/limits                                                             |
| ---------------------- | ------------------------------------ | ------------------------------------------- | --------------------------------------------------------------------------- |
| Generic Images         | JSON Images                          | multipart or explicit JSON/generation input | exact fixture mapping; capabilities configured per route                    |
| OmniRoute              | Images JSON                          | supported custom/Codex edits                | pinned source; Antigravity text-only, masks unsupported, Codex n>1 rejected |
| 9router                | Images JSON/binary/SSE parse         | selected verified single-ref route          | pinned source; Antigravity executor drops refs so rejected                  |
| Gemini generateContent | contents/inlineData                  | native references                           | native protocol, thought images excluded; mask/format not universal         |
| Gemini Interactions    | typed input/response_format          | native references                           | separate steps model_output, stateless default                              |
| OpenAI Responses       | image tool under orchestration model | input_image                                 | completed call results only; mask requires other documented route           |
| OpenRouter Images      | dedicated /images                    | input_references                            | dedicated model/endpoints descriptors; no automatic fallback                |
| Chat images            | opt-in chat/completions              | documented image_url input                  | explicit image output only; never gateway image-only models                 |
| ComfyUI                | explicit API graph                   | explicit reference/mask graph bindings      | node/GPU checks; job history/view, no automatic node/checkpoint install     |

Native count>1 requires documented maxCount and actual mapped batch support; not silently repeated.
PNG/JPEG/WebP originals validated. SVG/animation unsupported. Source references, live/GUI/GPU not-run
reasons and exact runtime/platform evidence are separate. SDK2.3.0 selected. Local tests verify legacy
initialize and auto negotiation falls back to legacy on this direct stdio server; a modern-only pin is
rejected. No modern-only transport support is advertised. Provider signatures are privately retained;
stateless file-reference editing is supported, conversational continuation replay is not exposed.

### OpenAI-compatible vs Anthropic-compatible API

Numera currently submits images through evidenced image APIs, including OpenAI-compatible Images
and Responses. Using Claude as the MCP host does not require an Anthropic image endpoint: Claude
calls Numera over MCP, and Numera calls your selected image backend independently.

The standard Anthropic POST /v1/messages contract accepts images for vision but does not define a
native generated-image output or OpenAI-style image_generation tool. A generic Anthropic-compatible
chat endpoint is therefore not sufficient for image generation. Numera does not claim such an adapter.

The examined OmniRoute3.8.51 Messages path delegates to chat; image-only models are rejected and its
Claude response translation does not preserve generated image content. Use its supported
/v1/images/generations route instead. A future Messages image-output adapter needs an explicit
gateway extension defining request options, authentication and final image bytes; no protocol
inference, silent parameter dropping or automatic fallback is permitted.

Sources: [Anthropic Messages](https://platform.claude.com/docs/en/api/messages/create),
[vision input](https://platform.claude.com/docs/en/build-with-claude/vision), and
[pinned OmniRoute response conversion](https://github.com/diegosouzapw/OmniRoute/blob/c1e30b7676975feb298b49eff6ff58923c04b89e/open-sse/handlers/responseTranslator.ts#L736).
