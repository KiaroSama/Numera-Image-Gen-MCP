# Numera Image Orchestration

A personal bridge between a tool host and explicitly selected image backends.

## Language

**Connection**: A named image destination with its own credential source and request policy.
_Avoid_: Provider account, globally shared API.

**Adapter**: A wire protocol for submitting and interpreting image work.
_Avoid_: Model family, gateway brand.

**Gateway profile**: A gateway's discovery and forwarding behavior layered on an adapter.
_Avoid_: Native model capability.

**Forwarding support**: What the chosen route actually carries to the upstream image model.
_Avoid_: Vision support, advertised image input.

**Capability evidence**: The source, scope and verification state of a support claim.
_Avoid_: Universal compatibility.

**Logical request**: One authorized image operation identified across host retries.
_Avoid_: Prompt cache entry.

**Receipt**: The durable outcome of a logical request, including uncertainty and saved outputs.
_Avoid_: Guaranteed successful generation.

**Owned output**: An image Numera saved and can verify by its own identifier and bytes.
_Avoid_: Arbitrary local file.

**Local cancellation**: Stopping Numera's waiting, distinct from upstream cancellation or refund.
_Avoid_: Refund confirmation.
