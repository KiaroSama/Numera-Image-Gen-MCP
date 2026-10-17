use crate::compat::value::Value;
use crate::{
    error::{NumeraError, Result},
    http::{bounded::BoundedBytes, sse, url::operation_url},
};
use reqwest::{Client, Method, header::HeaderValue, multipart::Form};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub enum Body {
    Json(Value),
    Multipart(Form),
    Empty,
}
pub struct Operation<'a> {
    pub operation: &'a str,
    pub prefix: &'a str,
    pub method: Method,
    pub body: Body,
    pub query: Option<&'a Value>,
}

pub struct ApiResponse {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub request_id: Option<String>,
}

pub fn client(timeout: Duration) -> reqwest::ClientBuilder {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(timeout)
}

pub async fn request(
    connection: &Value,
    headers: &HashMap<String, String>,
    wire: Operation<'_>,
    cancel: &CancellationToken,
    limit: usize,
) -> Result<ApiResponse> {
    request_observed(connection, headers, wire, cancel, limit, |_, _, _| {}).await
}

/// Observes preparation only; the callback does not establish that a request was sent.
pub async fn request_observed(
    connection: &Value,
    headers: &HashMap<String, String>,
    wire: Operation<'_>,
    cancel: &CancellationToken,
    limit: usize,
    prepared: impl FnOnce(u64, u64, u64) + Send,
) -> Result<ApiResponse> {
    let started = Instant::now();
    let Operation {
        operation,
        prefix,
        method,
        body,
        query,
    } = wire;
    let timeout = if method == Method::POST {
        connection["requestTimeoutMs"].as_u64().unwrap_or(300000)
    } else {
        connection["discoveryTimeoutMs"].as_u64().unwrap_or(15000)
    };
    let mut params = connection["query"].as_object().cloned().unwrap_or_default();
    if let Some(query) = query.and_then(Value::as_object) {
        params.extend(query.clone());
    }
    let mapped = match connection["paths"].get(operation) {
        Some(Value::String(path)) => String::from_utf16_lossy(path.as_utf16()),
        None => operation.to_owned(),
        _ => {
            return Err(NumeraError::new(
                "invalid_configuration",
                "Invalid configured operation path.",
            ));
        }
    };
    let url = operation_url(
        &String::from_utf16_lossy(
            connection["baseUrl"]
                .as_utf16()
                .ok_or_else(NumeraError::runtime)?,
        ),
        connection["baseUrlMode"].as_str().unwrap_or("api-prefix"),
        &mapped,
        prefix,
        &Value::Object(params),
    )?;
    let mut builder = client(Duration::from_millis(timeout));
    if let Some(proxy) = connection.get("proxy") {
        let mut proxy_agent = reqwest::Proxy::all(String::from_utf16_lossy(
            proxy["url"].as_utf16().ok_or_else(NumeraError::runtime)?,
        ))
        .map_err(|_| NumeraError::new("invalid_configuration", "Invalid proxy configuration."))?;
        if let Some(env) = proxy["secretEnv"].as_str()
            && let Ok(value) = std::env::var(env)
        {
            let header = HeaderValue::from_str(&value).map_err(|_| {
                NumeraError::new("invalid_configuration", "Invalid proxy credential.")
            })?;
            proxy_agent = proxy_agent.custom_http_auth(header);
        }
        builder = builder.proxy(proxy_agent);
    }
    let building = Instant::now();
    let client = builder.build().map_err(|_| {
        NumeraError::new(
            "invalid_configuration",
            "Cannot initialize configured API transport.",
        )
    })?;
    let client_build_ms = building.elapsed().as_millis() as u64;
    let expects_body = method != Method::HEAD;
    let mut request = client.request(method, url);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request = match body {
        Body::Json(value) => request
            .header("Content-Type", "application/json")
            .body(value.stringify()?),
        Body::Multipart(form) => request.multipart(form),
        Body::Empty => request,
    };
    prepared(
        client_build_ms,
        started.elapsed().as_millis() as u64,
        timeout,
    );
    let operation = async {
        let mut response = request.send().await.map_err(network_error)?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(
                NumeraError::new("unsafe_destination", "API redirects are not followed.")
                    .stage("submission"),
            );
        }
        if !(200..300).contains(&status) {
            let code = match status {
                401 => "authentication_error",
                403 => "permission_denied",
                429 => "rate_limited",
                404 => "model_unavailable",
                _ => "upstream_error",
            };
            let mut error = NumeraError::new(
                code,
                &format!("Upstream returned HTTP {status}. No automatic resubmission was made."),
            )
            .stage("upstream");
            error.http_status = Some(status);
            error.next_action="Check connection/account/model and inspect its existing job before authorizing a new request.".into();
            return Err(error);
        }
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = BoundedBytes::new(
            response.content_length(),
            expects_body && !matches!(status, 204 | 205),
            limit,
        )?;
        let mut stream = mime
            .contains("text/event-stream")
            .then(|| sse::Reader::new(limit));
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if let Some(stream) = stream.as_mut() {
                stream.push(&chunk)?;
            } else {
                bytes.push(&chunk, None)?;
            }
        }
        let bytes = if let Some(stream) = stream {
            stream.finish()?
        } else {
            bytes.finish()
        };
        Ok(ApiResponse {
            bytes,
            mime,
            request_id,
        })
    };
    tokio::select! {
        _=cancel.cancelled()=>Err(NumeraError::new("cancelled","Waiting stopped; upstream completion and billing may be unknown.").stage("network")),
        result=tokio::time::timeout(Duration::from_millis(timeout),operation)=>result.map_err(|_|NumeraError::new("request_timeout","Waiting stopped; upstream completion and billing may be unknown.").stage("network"))?,
    }
}

pub async fn json(
    connection: &Value,
    headers: &HashMap<String, String>,
    wire: Operation<'_>,
    cancel: &CancellationToken,
) -> Result<Value> {
    let response = request(connection, headers, wire, cancel, 16 * 1024 * 1024).await?;
    std::str::from_utf8(&response.bytes)
        .ok()
        .and_then(|text| Value::parse_with_limit(text, 16 * 1024 * 1024).ok())
        .ok_or_else(|| {
            NumeraError::new("invalid_response", "Upstream returned invalid JSON.")
                .stage("response")
        })
}

fn network_error(error: reqwest::Error) -> NumeraError {
    if error.is_timeout() {
        NumeraError::new(
            "request_timeout",
            "Waiting stopped; upstream completion and billing may be unknown.",
        )
        .stage("network")
    } else {
        NumeraError::new(
            "outcome_unknown",
            "Connection interrupted; do not automatically resubmit.",
        )
        .stage("network")
    }
}
