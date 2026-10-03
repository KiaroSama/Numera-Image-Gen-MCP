# Image-to-image, comic editing and in-image translation

Use generate_image for a user or agent prompt. Use edit_image with at least one approved reference for
image-to-image, comic changes or replacing text inside an image. The agent must select a model with
verified reference forwarding; a vision-capable model behind a text-only gateway cannot edit.

Sources are typed objects: output_id for a Numera-owned output, path under the configured inputs root,
data_url for PNG/JPEG/WebP bytes, or an explicitly allowed safe URL. Source bytes are forwarded unchanged.
No automatic upload, conversion, provider fallback or extra paid request is performed.

## Comic translation

```json
{
  "prompt": "Preserve the artwork and speech bubble layout.",
  "request_id": "comic-translation-001",
  "reference_images": [
    {
      "type": "path",
      "path": "C:/projects/numera-image-gen-mcp/inputs/comic.png"
    }
  ],
  "target_language": "Persian"
}
```

Call edit_image with this object. target_language explicitly asks Numera to append instructions to
translate and replace the text inside the image, preserving artwork/panels/text styling. It is not a
separate OCR/text result. Without target_language the prompt is forwarded unchanged. Model typography,
translation accuracy and preservation are not guaranteed; inspect the actual result. Do not submit a
private/licensed comic without permission to send it to the selected provider.

## Selected rectangle

Add edit_region to edit_image:

```json
{ "edit_region": { "x": 120, "y": 80, "width": 240, "height": 100 } }
```

Coordinates are absolute pixels from the top-left of the first reference, using its decoded dimensions.
x/y must be nonnegative integers; width/height positive integers. The entire rectangle must fit.
Numera generates a same-size PNG mask with the configured documented polarity. edit_region and an
explicit mask are mutually exclusive. Region editing/translation requires edit_image and references.

The selected route must actually accept masks; unsupported routes fail before paid submission.
A mask is the provider's edit boundary, not a guarantee it preserves every outside pixel. Native Gemini
references can support whole-image editing/translation but this adapter has no native mask contract.
Known OmniRoute/9router Antigravity image-input forwarding limitations still apply; no silent fallback.

## Remove an authorized watermark

For images you own or have permission to modify, call edit_image with a precise removal prompt and
edit_region enclosing the watermark. No separate tool, automatic watermark detector or extra upload
is needed. Example for a source at least1000x650 pixels:

```json
{
  "prompt": "Remove only the watermark inside the selected region. Reconstruct the background texture naturally and preserve the rest of the image.",
  "request_id": "watermark-removal-001",
  "reference_images": [
    {
      "type": "path",
      "path": "C:/projects/numera-image-gen-mcp/inputs/owned-image.png"
    }
  ],
  "edit_region": { "x": 800, "y": 550, "width": 200, "height": 100 }
}
```

Replace the example coordinates with the actual watermark bounds; never guess dimensions or silently
omit the region. Call get_model_capabilities first: the selected route must support references and
masks. If it does not, report that limitation rather than changing providers or submitting a whole-image
fallback. The model reconstructs covered pixels; removal quality and unchanged outside pixels are not
guaranteed. Inspect the saved result, and keep the original source.

## Upscale through your selected image model

Call edit_image with `upscale: true`, exactly one approved reference, a preservation prompt and
numeric `size`, such as `2048x2048` for a1024x1024 source. Both target dimensions must not shrink,
at least one must grow, and the target must fit configured pixel limits. Masks/regions/translation
cannot be combined with this mode. Numera appends explicit preservation/detail instructions and
uses the same selected model's verified edit route, never a local resize or another provider.

```json
{
  "prompt": "Preserve the subject, composition and colors.",
  "upscale": true,
  "size": "2048x2048",
  "reference_images": [
    { "type": "output_id", "output_id": "REPLACE_WITH_OWNED_OUTPUT_UUID" }
  ],
  "request_id": "native-upscale-001"
}
```

The selected route must support references and that native size. Unknown forwarding fails before
submission. A returned image not enlarged beyond the source, or differing from requested dimensions,
is partial with a deviation; no local resizing hides it. This is model-based re-rendering, not a
dedicated super-resolution engine. Fine-detail accuracy/pixel preservation and quality improvement
are not guaranteed. Keep the source and visually compare the result.

## Native dimensions only

Use size for provider-supported pixel dimensions (for example1024x1024), aspect_ratio for native shapes,
and image_size for native resolution tiers. These are different fields; do not substitute one for another.
Only parameters accepted by the selected adapter/route are forwarded. Numera never resizes final originals
to satisfy a requested size. If provider bytes have different dimensions/format, the receipt records the
deviation and is partial, rather than falsely reporting an exact result. Unknown account/model support
is not inferred from a listed model ID.

Offline fixtures verify masks, exact source bytes, payload instructions and error boundaries. They do
not prove semantic comic translation or all-provider editing quality; actual samples/account support
need separate authorized live validation.
