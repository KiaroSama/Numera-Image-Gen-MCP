# Compatibility evidence

Protocol compatibility is not a measured percentage of all models. Model names do not prove support.
Every live connection/model/account/size/input mode must be verified separately. Current status before
final CI is implemented, initial seam-tested; full matrix/coverage results belong in TEST-RESULTS.md.

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
reasons and exact runtime/platform evidence are separate. SDK2.2.0 selected; v1 wire/client negotiation
requires explicit legacy-client tests before advertising broad older-host compatibility.
