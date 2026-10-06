// Interpret verified gateway aliases for policy only; never rewrite the caller's wire model ID.
export function isCodexModel(model: string): boolean {
  return /^(?:codex|cx)\//.test(model);
}
