---
name: numera-image-gen
description: Generate or edit images through an installed Numera MCP connection; inspect capabilities, handle references, interpret outputs, and recover uncertain jobs without duplicate submission.
---

# Numera image workflow

1. Call list_connections and list_models/get_model_capabilities for the user's selected destination/model.
   Separate documented model support from gateway forwarding and account availability.
2. Obtain host/user authorization for the actual operation and cost. Keep the host chat model unchanged.
3. Generate one request_id per logical operation. Use typed approved references: output_id, path, url
   or data_url. Edit only with a known supported input route; no omitted refs/masks or hidden uploads.
4. Call generate_image/edit_image once with the original prompt and explicit effective settings.
   Reuse the same ID for host duplicate calls; a different ID is a new potentially charged operation.
5. Inspect receipt status, outputs, deviations and errors. Saved files/verified hashes are the baseline;
   preview rendering/local path access depends on the client.
6. For running/unknown outcomes call get_job, never blindly regenerate. cancel_job may stop local
   waiting without upstream cancellation or refund. Ask for new authorization before another submission.

No credentials belong in tool arguments. Respect host permissions and configured input roots.
The server works without this optional skill.
