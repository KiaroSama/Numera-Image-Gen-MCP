import { lookup } from "node:dns/promises";
import { isIP, BlockList } from "node:net";
import { Agent, fetch } from "undici";
import type { Config } from "../config/schema.js";
import { boundedBytes } from "./client.js";
import { fail } from "../errors.js";
const blocked = new BlockList();
for (const [network, bits] of [
  ["0.0.0.0", 8],
  ["10.0.0.0", 8],
  ["100.64.0.0", 10],
  ["127.0.0.0", 8],
  ["169.254.0.0", 16],
  ["172.16.0.0", 12],
  ["192.0.0.0", 24],
  ["192.168.0.0", 16],
  ["198.18.0.0", 15],
  ["224.0.0.0", 4],
  ["240.0.0.0", 4],
] as const)
  blocked.addSubnet(network, bits, "ipv4");
for (const [network, bits] of [
  ["::", 128],
  ["::1", 128],
  ["fc00::", 7],
  ["fe80::", 10],
  ["ff00::", 8],
  ["2001:db8::", 32],
] as const)
  blocked.addSubnet(network, bits, "ipv6");
export function privateAddress(address: string): boolean {
  const normalized = address.toLowerCase();
  if (normalized.startsWith("::ffff:")) {
    const tail = normalized.slice(7);
    if (isIP(tail) === 4) return privateAddress(tail);
    const parts = tail.split(":");
    if (parts.length === 2) {
      const n = parseInt(parts[0]!, 16) * 65536 + parseInt(parts[1]!, 16);
      return privateAddress(
        `${n >>> 24}.${(n >>> 16) & 255}.${(n >>> 8) & 255}.${n & 255}`,
      );
    }
  }
  return (
    !isIP(address) ||
    blocked.check(address, isIP(address) === 4 ? "ipv4" : "ipv6")
  );
}
export async function fetchAsset(
  raw: string,
  config: Config,
  limit: number,
  signal?: AbortSignal,
): Promise<Buffer> {
  const deadline = AbortSignal.timeout(30000),
    combined = signal ? AbortSignal.any([signal, deadline]) : deadline;
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return fail("unsafe_destination", "Invalid image URL.");
  }
  for (let hop = 0; hop < 4; hop++) {
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.hash
    )
      fail("unsafe_destination", "Unsafe image URL.");
    const allowance = config.files.assetAllowances.some(
      (a) =>
        new URL(a.origin).origin === url.origin &&
        (url.pathname === a.pathPrefix ||
          url.pathname.startsWith(
            a.pathPrefix.endsWith("/") ? a.pathPrefix : a.pathPrefix + "/",
          )),
    );
    const host = url.hostname.replace(/^\[|\]$/g, "");
    const addresses = isIP(host)
      ? [{ address: host, family: isIP(host) }]
      : await lookup(host, { all: true });
    if (
      !addresses.length ||
      (!allowance && addresses.some((a) => privateAddress(a.address)))
    )
      fail(
        "unsafe_destination",
        "Image URL resolves to a private, reserved or metadata destination.",
      );
    const address = addresses[0]!;
    const agent = new Agent({
      connect: {
        lookup: (_hostname, _options, callback) =>
          callback(null, address.address, address.family),
      },
    });
    try {
      const response = await fetch(url, {
        signal: combined,
        redirect: "manual",
        dispatcher: agent,
      });
      if (response.status >= 300 && response.status < 400) {
        const location = response.headers.get("location");
        await response.body?.cancel();
        if (!location)
          fail("unsafe_destination", "Redirect has no destination.");
        url = new URL(location, url);
        continue;
      }
      if (!response.ok) {
        await response.body?.cancel();
        fail(
          "input_file_error",
          `Image download returned HTTP ${response.status}.`,
          "download",
        );
      }
      return await boundedBytes(response, limit, combined);
    } finally {
      await agent.close();
    }
  }
  return fail("unsafe_destination", "Too many image redirects.");
}
