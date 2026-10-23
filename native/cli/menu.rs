use super::http_settings;
use crate::error::{NumeraError, Result};
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    StartHttp { port: u16 },
    Continue,
    Exit,
}

/// One synchronous interaction; the primary CLI dispatches StartHttp asynchronously in foreground.
/// No token values, provider configuration, host settings or background process are accessed here.
pub fn step<R: BufRead, W: Write>(
    root: &Path,
    env: &HashMap<String, String>,
    input: &mut R,
    output: &mut W,
) -> Result<MenuAction> {
    write!(output,"\nNumera Image-Gen MCP\n1. Start HTTP server\n2. Change port\n3. Show connection guidance\n4. Exit\nSelect [1]: ")
        .and_then(|_| output.flush()).map_err(|_| io_error())?;
    let Some(choice) = line(input)? else {
        return Ok(MenuAction::Exit);
    };
    match choice.trim() {
        "" | "1" => Ok(MenuAction::StartHttp {
            port: http_settings::port(root, &[], env)?,
        }),
        "2" => {
            write!(output, "New port (1-65535; blank cancels): ")
                .and_then(|_| output.flush())
                .map_err(|_| io_error())?;
            let Some(value) = line(input)? else {
                return Ok(MenuAction::Exit);
            };
            if value.is_empty() {
                return Ok(MenuAction::Continue);
            }
            match http_settings::parse_port(&value) {
                Ok(port) => {
                    http_settings::save_port(root, port)?;
                    writeln!(
                        output,
                        "Port saved. Restart an active HTTP server to apply the change."
                    )
                    .map_err(|_| io_error())?;
                }
                Err(_) => {
                    writeln!(
                        output,
                        "Invalid port. Enter a decimal integer between 1 and 65535."
                    )
                    .map_err(|_| io_error())?;
                }
            }
            Ok(MenuAction::Continue)
        }
        "3" => {
            let port = http_settings::port(root, &[], env)?;
            writeln!(output, "Connection URL: http://127.0.0.1:{port}/mcp")
                .map_err(|_| io_error())?;
            if env.contains_key("NUMERA_HTTP_TOKEN") {
                writeln!(output,"Authentication: use the private NUMERA_HTTP_TOKEN environment value; it is not displayed.").map_err(|_|io_error())?;
            } else {
                writeln!(output,"Authentication: Bearer token from {} (created when HTTP starts; never displayed here).",http_settings::token_path(root)?.display()).map_err(|_|io_error())?;
            }
            writeln!(output,"Configure only the requested Numera client entry with this URL and its private Authorization header. Do not replace unrelated host settings.\nStdio clients may launch the native binary directly with args [\"bridge\"].").map_err(|_|io_error())?;
            Ok(MenuAction::Continue)
        }
        "4" => Ok(MenuAction::Exit),
        _ => {
            writeln!(output, "Invalid selection. Choose 1, 2, 3 or 4.").map_err(|_| io_error())?;
            Ok(MenuAction::Continue)
        }
    }
}

fn line<R: BufRead>(input: &mut R) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = input.fill_buf().map_err(|_| io_error())?;
        if available.is_empty() {
            break;
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |n| n + 1);
        if bytes.len().checked_add(count).is_none_or(|n| n > 1024) {
            return Err(NumeraError::new(
                "invalid_input",
                "Menu input exceeds the 1024-byte limit.",
            ));
        }
        bytes.extend_from_slice(&available[..count]);
        let complete = available[count - 1] == b'\n';
        input.consume(count);
        if complete {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| NumeraError::new("invalid_input", "Menu input must be UTF-8."))
}
fn io_error() -> NumeraError {
    NumeraError::new("input_file_error", "Cannot read or write the native menu.").stage("menu")
}
