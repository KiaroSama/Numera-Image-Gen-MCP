import { fingerprint } from "../jobs/store.js";
import type { Logger } from "../logging.js";
import type { Config } from "./schema.js";
import { loadConfig } from "./load.js";
import { readConfigText } from "./env.js";

function fixedSettings(config: Config) {
  return fingerprint(
    Object.fromEntries(
      Object.entries(config).filter(
        ([key]) =>
          !["connections", "defaultConnection", "returnMode"].includes(key),
      ),
    ),
  );
}

export class ConfigReload {
  private lastSeen: string | undefined;
  private serial: Promise<Config>;
  constructor(
    private current: Config,
    readonly file: string,
    private args: string[],
    private env: NodeJS.ProcessEnv,
    private logger: Logger,
  ) {
    this.serial = Promise.resolve(current);
  }
  refresh(
    accept: (candidate: Config) => boolean = () => true,
  ): Promise<Config> {
    this.serial = this.serial.then(async () => {
      let hash = "unreadable";
      try {
        const text = await readConfigText(this.file);
        hash = fingerprint(text);
        if (hash === this.lastSeen) return this.current;
        this.lastSeen = hash;
        const candidate = await loadConfig(this.args, this.env, text);
        if (fixedSettings(candidate) !== fixedSettings(this.current)) {
          this.logger.log(
            "WARNING",
            "config",
            "Saved configuration rejected; storage, logging, file limits and global policy changes require restart.",
          );
          return this.current;
        }
        if (fingerprint(candidate) !== fingerprint(this.current)) {
          if (!accept(candidate)) {
            this.logger.log(
              "WARNING",
              "config",
              "Saved configuration rejected; retained snapshot limit requires restart.",
            );
            return this.current;
          }
          this.current = candidate;
          this.logger.log("INFO", "config", "Saved configuration accepted.");
        }
      } catch {
        if (hash !== this.lastSeen || hash !== "unreadable") {
          this.lastSeen = hash;
          this.logger.log(
            "WARNING",
            "config",
            "Saved configuration rejected; retaining last valid configuration.",
          );
        }
      }
      return this.current;
    });
    return this.serial;
  }
}
