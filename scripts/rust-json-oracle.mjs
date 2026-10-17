// Keep raw JSON text: serializing the parsed input would erase nonfinite numbers before comparison.
export async function jsonOracle({
  emit,
  fingerprint,
  configSchema,
  requestSchema,
}) {
  const texts = [
    ["overflow-values", '{"a":1e400,"b":-1e400,"c":1e-400}'],
    ["lone-surrogate-value", '{"text":"\\ud800"}'],
    ["lone-surrogate-key", '{"\\udfff":"key","ordinary":1}'],
    ["paired-surrogate", '{"text":"\\ud83c\\udf0d"}'],
    ["duplicate-keys", '{"a":1,"b":2,"a":3}'],
    ["binary64-rounded-integer", '{"number":9007199254740993}'],
  ];
  const stable = (value) =>
    Array.isArray(value)
      ? value.map(stable)
      : value && typeof value === "object"
        ? Object.fromEntries(
            Object.entries(value)
              .sort(([a], [b]) => a.localeCompare(b, "en"))
              .map(([key, item]) => [key, stable(item)]),
          )
        : value;
  const cases = texts.map(([name, text]) => {
    const value = JSON.parse(text);
    return {
      name,
      text,
      serialized: JSON.stringify(value),
      canonical: JSON.stringify(stable(value)),
      sha256: fingerprint(value),
    };
  });
  await emit("raw-json", cases);
  const schemaCases = [];
  const add = (name, schema, text) => {
    const parsed = (
      schema === "config" ? configSchema : requestSchema
    ).safeParse(JSON.parse(text));
    schemaCases.push({
      name,
      schema,
      text,
      valid: parsed.success,
      serialized: parsed.success ? JSON.stringify(parsed.data) : null,
    });
  };
  add(
    "request-opaque-utf16-and-overflow",
    "image-request",
    '{"prompt":"\\ud800","provider_options":{"\\udfff":1e400,"nested":[-1e400]}}',
  );
  add(
    "request-url-surrogate-validation",
    "image-request",
    '{"prompt":"draw","reference_images":[{"type":"url","url":"https://example.com/\\ud800"}]}',
  );
  for (const id of [
    "00000000-0000-0000-0000-000000000000",
    "ffffffff-ffff-ffff-ffff-ffffffffffff",
    "12345678-1234-4234-8234-123456789abc",
    "12345678-1234-4234-1234-123456789abc",
    "12345678-1234-9234-8234-123456789abc",
  ]) {
    add(
      "request-uuid-" + id,
      "image-request",
      JSON.stringify({
        prompt: "draw",
        reference_images: [{ type: "output_id", output_id: id }],
      }),
    );
  }
  add(
    "config-opaque-utf16-and-overflow",
    "config",
    '{"schemaVersion":1,"outputDir":"/tmp/numera-outputs","stateDir":"/tmp/numera-state","connections":{"default":{"adapter":"openai-images","baseUrl":"https://example.com/v1","auth":{"type":"none"},"defaults":{"\\udfff":1e400,"text":"\\ud800"}}}}',
  );
  await emit("raw-js-schema", schemaCases);
  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(":memory:");
  try {
    db.exec("CREATE TABLE values_table(value TEXT)");
    const values = [];
    for (const text of ['"https://example.com/\\ud800"', '"🌍"']) {
      const value = JSON.parse(text);
      db.prepare("INSERT INTO values_table VALUES(?)").run(value);
      const row = db
        .prepare(
          "SELECT value,hex(CAST(value AS BLOB)) AS bytes FROM values_table ORDER BY rowid DESC LIMIT 1",
        )
        .get();
      values.push({
        text,
        stored: row.value,
        bytes: row.bytes,
        utf8_bytes: Buffer.byteLength(value, "utf8"),
      });
    }
    await emit("sqlite-utf16-text", values);
  } finally {
    db.close();
  }
}
