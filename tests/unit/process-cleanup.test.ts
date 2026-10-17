import { describe, expect, it } from "vitest";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

type Failure = { phase: string; code: string };
type Proof = {
  closed: boolean;
  groupAbsent: boolean;
  survivors: number[];
  failures: Failure[];
};
const { cleanupProof } = (await import(
  pathToFileURL(resolve("scripts/bounded.mjs")).href
)) as {
  cleanupProof(input: Proof): {
    verified: boolean;
    failures: Failure[];
    resolved: (Failure & { resolution: string })[];
  };
};

const denied = { phase: "group_kill", code: "EPERM" };
const input = (changes: Partial<Proof> = {}): Proof => ({
  closed: true,
  groupAbsent: true,
  survivors: [],
  failures: [denied],
  ...changes,
});

describe("owned process cleanup final-state proof", () => {
  it("resolves a post-close denied kill only after definitive group absence", () => {
    expect(cleanupProof(input())).toEqual({
      verified: true,
      failures: [],
      resolved: [{ ...denied, resolution: "closed_group_absent" }],
    });
  });

  it.each([
    { closed: false },
    { groupAbsent: false },
    { survivors: [314] },
    { failures: [denied, { phase: "group_probe", code: "EPERM" }] },
    { failures: [denied, { phase: "group_deadline", code: "ETIMEDOUT" }] },
    { failures: [{ phase: "group_kill", code: "EACCES" }] },
  ])(
    "keeps unresolved closure, permission, survivor or deadline failures: %j",
    (changes) => {
      expect(cleanupProof(input(changes)).verified).toBe(false);
    },
  );
});
