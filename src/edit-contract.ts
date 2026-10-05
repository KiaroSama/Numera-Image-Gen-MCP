import { fail } from "./errors.js";
import type { Connection } from "./config/schema.js";

export function validateEditContract(edit: Connection["edit"], count?: number) {
  if (!edit) return;
  if (edit.mode === "multipart") {
    if (!["image", "image[]"].includes(edit.encoding))
      fail(
        "invalid_configuration",
        "Multipart editing requires image or image[] encoding.",
      );
    return;
  }
  if (edit.encoding === "image[]")
    fail(
      "invalid_configuration",
      "JSON editing cannot use multipart image[] encoding.",
    );
  if (edit.encoding === "image" && (edit.maxReferences > 1 || (count ?? 0) > 1))
    fail(
      "unsupported_operation",
      "Scalar image encoding supports exactly one reference; use a verified array encoding.",
    );
}
