import { NumeraError } from "../errors.js";
import type { TerminalState } from "../jobs/lifecycle.js";

export class TerminalProviderError extends NumeraError {
  constructor(
    public terminal: TerminalState,
    message: string,
  ) {
    super("provider_rejection", message, "generation");
  }
}
