# Direct local HTTP MCP

Provider-free hosted native SDK/CLI and packaged HTTP checks passed on all six targets at
`7199441`. Complete migration acceptance, a verified Release and actual owner GUI registration remain
pending. Do not replace a working client entry until a verified binary Release is installed.

## Start

From the extracted installation root on Linux/macOS:

```bash
./bin/numera-image-gen-mcp http
```

Windows PowerShell:

```powershell
& './bin/numera-image-gen-mcp.exe' http
```

The endpoint is `http://127.0.0.1:1969/mcp`. It serves the same ten tools/resources and durable
receipts as `bridge`. It is not a proxy to another MCP service. The listener is foreground;
Ctrl+C stops it. No background service, firewall change or public-network listener is installed.
The stdio `bridge` remains independent and requires no running HTTP server or launcher.

## Native menu and configurable port

```bash
./bin/numera-image-gen-mcp menu
```

Windows uses the same `menu` argument with its `.exe`. The menu can start HTTP, change its port,
show connection guidance or exit. Enter selects HTTP start; EOF exits. Running without arguments
from an interactive terminal selects the menu; non-interactive no-argument launch remains stdio.

```bash
./bin/numera-image-gen-mcp http --port 1970
```

Precedence: `--port`, then `NUMERA_HTTP_PORT`, then the menu-saved `config/http.json`, then1969.
Ports must be decimal integers1..65535. A busy port fails; Numera never chooses another silently.
Save changes through the menu and restart an active listener. Provider `.env`/JSON config and model
settings remain separate and unchanged. Existing `--config ABSOLUTE_PATH` remains available.

## Local authentication

HTTP requires a **separate local Bearer token**, never your image-provider API key. On first HTTP
startup Numera creates a private `config/http-token` beneath the installation root. Alternatively,
set `NUMERA_HTTP_TOKEN` to32..256 visible ASCII characters. Tokens are not printed, logged or included
in published archives. Read the private file locally when configuring your HTTP-capable client.

Configure the client URL and an `Authorization: Bearer <YOUR_PRIVATE_LOCAL_TOKEN>` header. The value
above is a placeholder, not a usable token. Keep the real value in the client's supported private
credential storage. Do not put it in a public config example, URL, command argument or support log.

Requests must target the exact loopback Host/port. A present browser Origin must match the same
loopback origin. Cross-origin/browser access, credential queries and arbitrary endpoint paths are
rejected; no wildcard CORS is enabled. This feature does not provide public hosting or OAuth login.

## Client differences

HTTP-capable clients can use the URL directly while the listener runs. Claude Desktop versions that
only accept local process entries should keep the documented absolute executable with
`args: ["bridge"]`; changing a URL cannot make that client support another transport.
See [Claude clients](CLAUDE.md). Actual GUI acceptance remains separate from SDK fixture evidence.

The requested references informed the flow, not the protocol: Agensi has a local HTTP relay to its
hosted service; Upwork is a stdio bridge to remote HTTP, and its local proxy port is not an MCP
listener. Numera's HTTP endpoint hosts Numera's own image tools. No reference credentials, OAuth
sessions, proxy catalogs or account features are copied.

## Limits and verification

Bodies and header reads are bounded; active requests/streams and sessions are capped. Shutdown
cancels local waiting but proves no upstream cancellation or refund. Recover existing logical IDs
with `get_job`, never resubmit uncertain paid work. Original image bytes and the existing journal
remain shared across transports.

Hosted Windows/Linux/macOS native matrices must exercise actual HTTP SDK clients, authentication,
Host/Origin rejection, ports/collisions, protocol/session handling, menu flows and shutdown. New
fixtures remain unverified until their exact-SHA CI results pass. No Rust build is required locally.
