import { parseArgs } from "node:util";
import { isAbsolute, join } from "node:path";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { loadConfig, selectConnection } from "../dist/config/load.js";
import { maintenance, projectRoot } from "./logging.mjs";

await maintenance("readiness", async (log) => {
  const { values } = parseArgs({
    options: {
      config: { type: "string" },
      connection: { type: "string" },
      model: { type: "string" },
      probe: { type: "boolean", default: false },
    },
    strict: true,
  });
  if (
    !values.config ||
    !isAbsolute(values.config) ||
    (values.model !== undefined &&
      (!values.model.trim() || values.model.length > 1024))
  )
    throw Object.assign(
      new Error("An absolute configuration and nonempty model are required."),
      { code: "INVALID_REQUEST" },
    );
  const config = await loadConfig(["--config", values.config]);
  const [name] = selectConnection(config, values.connection);
  const client = new Client({ name: "numera-readiness", version: "1.0.0" });
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [join(projectRoot, "dist/index.js"), "--config", values.config],
    env: { ...process.env },
    stderr: "pipe",
  });
  const blockers = [];
  const report = {
    checked_at: new Date().toISOString(),
    tool_count: 0,
    local_ready: false,
    credentials_ready: false,
    connection_ready: null,
    catalog_count: null,
    catalog_complete: null,
    selected_model_found: null,
    generation_verified: false,
    blockers,
  };
  const timer = setTimeout(() => {
    void transport.close();
  }, 60000);
  const call = async (tool, args) => {
    log.emit("INFO", "Read-only readiness step.", { tool });
    const result = await client.callTool(
      { name: tool, arguments: args },
      { timeout: 10000 },
    );
    if (result.isError || !result.structuredContent) {
      blockers.push("read_only_tool_failed");
      return {};
    }
    return result.structuredContent;
  };
  try {
    await client.connect(transport);
    transport.stderr?.resume();
    const tools = await client.listTools({}, { timeout: 10000 });
    report.tool_count = tools.tools.length;
    const expected = [
      "health_check",
      "list_connections",
      "list_models",
      "get_model_capabilities",
    ];
    if (
      report.tool_count !== 10 ||
      expected.some((n) => !tools.tools.some((t) => t.name === n))
    )
      blockers.push("tool_contract_mismatch");
    const health = await call("health_check", { probe: false });
    report.local_ready =
      health.configuration_valid === true &&
      Object.values(health.storage ?? {}).length === 3 &&
      Object.values(health.storage).every((v) => v === true);
    if (!report.local_ready)
      blockers.push("local_storage_or_configuration_blocked");
    const connections = await call("list_connections", {});
    report.credentials_ready =
      connections.connections?.find((c) => c.name === name)
        ?.credentials_ready === true;
    if (!report.credentials_ready)
      blockers.push("declared_credentials_missing");
    if (values.probe && report.credentials_ready) {
      const catalog = await call("list_models", {
        connection: name,
        refresh: true,
        limit: 100,
        offset: 0,
      });
      report.catalog_count = Array.isArray(catalog.models)
        ? catalog.models.length
        : 0;
      report.catalog_complete = catalog.complete === true;
      report.connection_ready =
        report.catalog_complete && report.catalog_count > 0;
      if (!report.connection_ready)
        blockers.push("catalog_incomplete_or_empty");
      if (values.model) {
        report.selected_model_found =
          catalog.models?.some((m) => m.id === values.model) === true;
        await call("get_model_capabilities", {
          connection: name,
          model: values.model,
        });
        if (!report.selected_model_found)
          blockers.push("model_not_in_first_catalog_page");
      }
    } else if (values.probe) report.connection_ready = false;
  } catch {
    blockers.push("protocol_or_read_only_probe_failed");
  } finally {
    clearTimeout(timer);
    try {
      await client.close();
    } finally {
      await transport.close();
    }
  }
  const ready =
    report.local_ready && report.credentials_ready && blockers.length === 0;
  process.stdout.write(JSON.stringify(report) + "\n");
  log.emit(
    ready ? "INFO" : "WARNING",
    "Readiness complete; generation was not tested.",
    {
      local_ready: report.local_ready,
      connection_ready: report.connection_ready,
      blocker_count: blockers.length,
    },
  );
  if (!ready) process.exitCode = 2;
});
