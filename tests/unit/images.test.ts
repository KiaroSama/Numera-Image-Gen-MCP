import { describe, expect, it } from "vitest";
import { createHash, randomUUID } from "node:crypto";
import {
  mkdir,
  readFile,
  readdir,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import sharp from "sharp";
import {
  dataUrl,
  decodeBase64,
  inspectImage,
  preview,
  validateMask,
} from "../../src/files/images.js";
import {
  contained,
  inputFile,
  outputDirectory,
  safeRelative,
} from "../../src/files/paths.js";
import { ownedImage, saveImage } from "../../src/files/output.js";
import { privateAddress } from "../../src/http/assets.js";
import { boundedBytes } from "../../src/http/client.js";
import { Store } from "../../src/jobs/store.js";
import { configuration, imageBytes, workspace } from "../fixtures/runtime.js";

const config = () =>
  configuration(join(process.cwd(), ".ci-work", "image-validation"));

describe("image bytes and previews", () => {
  it.each(["png", "jpeg", "webp"] as const)(
    "decodes real %s bytes with exact metadata, hash and Base64 round trip",
    async (format) => {
      const bytes = await imageBytes(format);
      const image = await inspectImage(bytes, config());
      expect(image).toMatchObject({
        mime: `image/${format}`,
        width: 8,
        height: 6,
        alpha: format !== "jpeg",
        extension: format === "jpeg" ? "jpg" : format,
      });
      expect(image.sha256).toBe(
        createHash("sha256").update(bytes).digest("hex"),
      );
      expect(image.bytes).toEqual(bytes);
      expect(decodeBase64(bytes.toString("base64"), bytes.length)).toEqual(
        bytes,
      );
      expect(
        dataUrl(
          `data:image/${format};base64,${bytes.toString("base64")}`,
          bytes.length,
        ),
      ).toEqual(bytes);
      expect(() =>
        decodeBase64(bytes.toString("base64"), bytes.length - 1),
      ).toThrow(expect.objectContaining({ code: "invalid_response" }));
    },
  );

  it.each(["", "%%%%", "YQ", "YQ===", "Y Q==", "YQ==\n"])(
    "rejects malformed or empty Base64 %j",
    (value) => {
      expect(() => decodeBase64(value, 20)).toThrow(
        expect.objectContaining({ code: "invalid_response" }),
      );
    },
  );

  it.each([
    "data:image/svg+xml;base64,YQ==",
    "data:image/png,YQ==",
    "data:text/plain;base64,YQ==",
    "https://example.com/image.png",
  ])("rejects unsupported data URL %s", (value) => {
    expect(() => dataUrl(value, 20)).toThrow(
      expect.objectContaining({ code: "invalid_input" }),
    );
  });

  it("rejects excessive pixels/bytes, truncated containers and unsupported signatures", async () => {
    const bytes = await imageBytes();
    await expect(
      inspectImage(bytes, config(), bytes.length - 1),
    ).rejects.toMatchObject({ code: "invalid_input" });
    const limited = config();
    limited.files.maxPixels = 47;
    await expect(inspectImage(bytes, limited)).rejects.toMatchObject({
      code: "invalid_input",
    });
    for (const invalid of [
      Buffer.alloc(0),
      Buffer.from("<svg/>", "utf8"),
      bytes.subarray(0, 20),
    ])
      await expect(inspectImage(invalid, config())).rejects.toMatchObject({
        code: "invalid_input",
      });
  });

  it("rejects actual animated WebP rather than accepting the first frame", async () => {
    const pixels = Buffer.from([255, 0, 0, 255, 0, 255, 0, 255]);
    const bytes = await sharp(pixels, {
      raw: { width: 1, height: 2, channels: 4, pageHeight: 1 },
    })
      .webp({ lossless: true, delay: [50, 50], loop: 0 })
      .toBuffer();
    expect((await sharp(bytes, { animated: true }).metadata()).pages).toBe(2);
    await expect(inspectImage(bytes, config())).rejects.toMatchObject({
      code: "invalid_input",
    });
  });

  it("requires a PNG alpha mask exactly matching the first source dimensions", async () => {
    const source = await inspectImage(await imageBytes(), config());
    await expect(validateMask(source, source)).resolves.toBeUndefined();
    const invalid = [
      await imageBytes("jpeg"),
      await imageBytes("png", false),
      await imageBytes("png", true, 7, 6),
      await imageBytes("png", true, 8, 5),
    ];
    for (const bytes of invalid)
      await expect(
        validateMask(await inspectImage(bytes, config()), source),
      ).rejects.toMatchObject({ code: "invalid_input" });
  });

  it("returns independently decodable bounded PNG previews without enlarging originals", async () => {
    const options = config();
    options.files.previewMaxDimension = 4;
    const result = await preview(
      await inspectImage(await imageBytes("webp"), options),
      options,
    );
    const bytes = Buffer.from(result.data, "base64");
    const metadata = await sharp(bytes).metadata();
    expect(result).toMatchObject({
      mimeType: "image/png",
      width: 4,
      height: 3,
    });
    expect(metadata).toMatchObject({ format: "png", width: 4, height: 3 });
    expect(bytes.length).toBeLessThanOrEqual(options.files.previewMaxBytes);
    options.files.previewMaxDimension = 100;
    expect(
      await preview(await inspectImage(await imageBytes(), options), options),
    ).toMatchObject({ width: 8, height: 6 });
    options.files.previewMaxDimension = 1;
    expect(
      await preview(await inspectImage(await imageBytes(), options), options),
    ).toMatchObject({ width: 1, height: 1 });
    options.files.previewMaxBytes = 1;
    await expect(
      preview(await inspectImage(await imageBytes(), options), options),
    ).rejects.toMatchObject({ code: "output_file_error", stage: "preview" });
  });
});

describe("contained filesystem input and output", () => {
  it.each([
    "",
    "../escape",
    "a/../b",
    "./a",
    "/absolute",
    "a//b",
    "a\\..\\b",
    "CON.png",
    "lpt1",
    "a.",
    "a ",
    "a:stream",
    "a\0b",
    "a?b",
  ])("rejects unsafe relative path %j", (path) => {
    expect(() => safeRelative(path)).toThrow(
      expect.objectContaining({ code: "invalid_input" }),
    );
  });

  it("allows nested Unicode paths and checks sibling containment rather than string prefixes", () => {
    expect(safeRelative("batch/café-output")).toBe("batch/café-output");
    const root = join(process.cwd(), ".ci-work", "allowed");
    expect(contained(root, root)).toBe(true);
    expect(contained(root, join(root, "a.png"))).toBe(true);
    expect(contained(root, join(root, "..", "allowed-other", "a.png"))).toBe(
      false,
    );
  });

  it("reads absolute/file URL inputs only inside canonical allowlisted roots and byte limits", async () =>
    workspace(async (root) => {
      const inputs = join(root, "inputs");
      await mkdir(inputs);
      const bytes = await imageBytes();
      const path = join(inputs, "café image.png");
      await writeFile(path, bytes);
      expect(await inputFile(path, [inputs], bytes.length)).toEqual(bytes);
      expect(
        await inputFile(pathToFileURL(path).href, [inputs], bytes.length),
      ).toEqual(bytes);
      await expect(
        inputFile(path, [join(root, "not-created")], bytes.length),
      ).rejects.toMatchObject({ code: "permission_denied" });
      await expect(
        inputFile(path, [inputs], bytes.length - 1),
      ).rejects.toMatchObject({ code: "input_file_error" });
      await expect(
        inputFile(inputs, [inputs], bytes.length),
      ).rejects.toMatchObject({ code: "input_file_error" });
      for (const invalid of [
        "relative.png",
        "file:///bad%ZZ",
        join(inputs, "absent.png"),
        `${path}:stream`,
      ])
        await expect(
          inputFile(invalid, [inputs], bytes.length),
        ).rejects.toMatchObject({ code: "input_file_error" });
    }));

  it("rejects input symlinks escaping roots and output junctions/non-directories", async () =>
    workspace(async (root) => {
      const inputs = join(root, "inputs"),
        outside = join(root, "outside"),
        outputs = join(root, "outputs");
      await Promise.all([mkdir(inputs), mkdir(outside), mkdir(outputs)]);
      await writeFile(join(outside, "image.png"), await imageBytes());
      await symlink(
        outside,
        join(inputs, "link"),
        process.platform === "win32" ? "junction" : "dir",
      );
      await expect(
        inputFile(join(inputs, "link", "image.png"), [inputs], 1000),
      ).rejects.toMatchObject({ code: "permission_denied" });
      await symlink(
        outside,
        join(outputs, "link"),
        process.platform === "win32" ? "junction" : "dir",
      );
      await expect(
        outputDirectory(outputs, "link/subdir"),
      ).rejects.toMatchObject({ code: "output_file_error" });
      await writeFile(join(outputs, "not-directory"), "fixture", "utf8");
      await expect(
        outputDirectory(outputs, "not-directory/subdir"),
      ).rejects.toMatchObject({ code: "output_file_error" });
      expect(await outputDirectory(outputs, "nested/café")).toBe(
        join(outputs, "nested", "café"),
      );
      await expect(outputDirectory(outputs, "../escape")).rejects.toMatchObject(
        { code: "invalid_input" },
      );
    }));

  it("saves collision-free verified images and refuses tampered/missing owned outputs", async () =>
    workspace(async (root) => {
      const options = configuration(root),
        store = new Store(options.stateDir);
      try {
        const image = await inspectImage(await imageBytes(), options);
        const first = await saveImage(
          image,
          options,
          store,
          "receipt",
          0,
          "batch",
          "café",
        );
        const second = await saveImage(
          image,
          options,
          store,
          "receipt",
          0,
          "batch",
          "café",
        );
        expect(first.path).not.toBe(second.path);
        expect(first).toMatchObject({
          request_id: "receipt",
          mime_type: "image/png",
          width: 8,
          height: 6,
          bytes: image.bytes.length,
          sha256: image.sha256,
        });
        expect(await readFile(first.path)).toEqual(image.bytes);
        expect(
          (await ownedImage(options, store, first.output_id)).image.sha256,
        ).toBe(image.sha256);
        expect(
          (await readdir(join(options.outputDir, "batch"))).sort(),
        ).toHaveLength(2);
        await writeFile(first.path, await imageBytes("png", true, 9, 6));
        await expect(
          ownedImage(options, store, first.output_id),
        ).rejects.toMatchObject({ code: "output_file_error" });
        await rm(second.path);
        await expect(
          ownedImage(options, store, second.output_id),
        ).rejects.toMatchObject({ code: "output_file_error" });
        await expect(
          saveImage(
            image,
            options,
            store,
            "receipt",
            1,
            "batch",
            "nested/prefix",
          ),
        ).rejects.toMatchObject({ code: "invalid_input" });
        await expect(
          saveImage(image, options, store, "receipt", 1, "../escape"),
        ).rejects.toMatchObject({ code: "invalid_input" });
        const outside = join(root, "outside.png");
        await writeFile(outside, image.bytes);
        const id = randomUUID();
        store.addOutput({ ...first, output_id: id, path: outside });
        await expect(ownedImage(options, store, id)).rejects.toMatchObject({
          code: "permission_denied",
        });
      } finally {
        store.close();
      }
    }));

  it("reports storage failure when output root is a regular file", async () =>
    workspace(async (root) => {
      const options = configuration(root),
        store = new Store(options.stateDir);
      try {
        await writeFile(options.outputDir, "blocked", "utf8");
        await expect(
          saveImage(
            await inspectImage(await imageBytes(), options),
            options,
            store,
            "receipt",
            0,
          ),
        ).rejects.toThrow();
        expect(store.listOutputs()).toEqual([]);
      } finally {
        store.close();
      }
    }));
});

describe("bounded download bytes and destination classification", () => {
  it.each([
    "127.0.0.1",
    "10.0.0.1",
    "169.254.169.254",
    "192.168.1.1",
    "100.64.0.1",
    "::1",
    "fc00::1",
    "fe80::1",
    "::ffff:127.0.0.1",
    "::ffff:7f00:1",
    "not-an-ip",
  ])("blocks private or invalid address %s", (address) => {
    expect(privateAddress(address)).toBe(true);
  });
  it.each(["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"])(
    "recognizes public address %s without network calls",
    (address) => {
      expect(privateAddress(address)).toBe(false);
    },
  );

  it("enforces advertised and actual stream limits, missing bodies and cancellation", async () => {
    const signal = new AbortController().signal;
    const body = async function* () {
      yield Uint8Array.from([1, 2]);
      yield Uint8Array.from([3]);
    };
    const response = { body: body(), headers: { get: () => null } };
    expect(await boundedBytes(response, 3, signal)).toEqual(
      Buffer.from([1, 2, 3]),
    );
    await expect(
      boundedBytes({ body: body(), headers: { get: () => "4" } }, 3, signal),
    ).rejects.toMatchObject({ code: "invalid_response" });
    await expect(
      boundedBytes({ body: body(), headers: { get: () => null } }, 2, signal),
    ).rejects.toMatchObject({ code: "invalid_response" });
    await expect(
      boundedBytes({ body: null, headers: { get: () => null } }, 3, signal),
    ).rejects.toMatchObject({ code: "invalid_response" });
    const aborted = AbortSignal.abort(new Error("fixture cancellation"));
    await expect(
      boundedBytes({ body: body(), headers: { get: () => null } }, 3, aborted),
    ).rejects.toThrow("fixture cancellation");
  });
});
