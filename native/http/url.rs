use crate::compat::value::Value;
use crate::error::{NumeraError, Result};
use url::Url;

pub fn operation_url(
    base: &str,
    mode: &str,
    operation: &str,
    prefix: &str,
    query: &Value,
) -> Result<Url> {
    let error = |message: &str| NumeraError::new("invalid_configuration", message);
    let mut url = Url::parse(base).map_err(|_| error("Invalid API base URL."))?;
    let lower = base.to_ascii_lowercase();
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || ["%2e", "%2f", "%5c"].iter().any(|s| lower.contains(s))
        || base.split('/').any(|s| s == "..")
    {
        return Err(error(
            "API base must not contain credentials, query, fragments or traversal.",
        ));
    }
    let path = url.path().strip_suffix('/').unwrap_or(url.path());
    if [
        "generations",
        "edits",
        "generateContent",
        "responses",
        "interactions",
        "images",
        "chat/completions",
    ]
    .iter()
    .any(|ending| path.ends_with(&format!("/{ending}")))
    {
        return Err(error("Supply an API prefix, not a full endpoint URL."));
    }
    if mode == "origin" && url.path() != "/" {
        return Err(error("Origin mode requires an origin without a path."));
    }
    if operation.is_empty()
        || operation.starts_with('/')
        || operation.chars().any(|c| "\\?#\0".contains(c))
        || operation.split('/').any(|part| {
            let decoded = percent_encoding::percent_decode_str(part).decode_utf8();
            decoded.is_err() || decoded.is_ok_and(|v| v == "." || v == "..")
        })
    {
        return Err(error("Invalid relative operation path."));
    }
    let path = format!(
        "{}/{}",
        if mode == "origin" {
            prefix.strip_suffix('/').unwrap_or(prefix)
        } else {
            url.path().strip_suffix('/').unwrap_or(url.path())
        },
        operation
    );
    url.set_path(&path);
    if let Some(query) = query.as_object() {
        for (key, value) in query {
            if key != "pageToken"
                && ["key", "token", "secret", "auth", "signature"]
                    .iter()
                    .any(|word| key.contains_ascii_case_insensitive(word))
            {
                return Err(error("Credentials must not be query parameters."));
            }
            let value = value
                .as_utf16()
                .ok_or_else(|| error("Invalid query parameter."))?;
            // URLSearchParams applies USVString coercion at this URL boundary only.
            url.query_pairs_mut().append_pair(
                &String::from_utf16_lossy(key.as_utf16()),
                &String::from_utf16_lossy(value),
            );
        }
    }
    Ok(url)
}
