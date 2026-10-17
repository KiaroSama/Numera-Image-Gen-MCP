use super::{application::Application, handler::Handler};
use crate::error::{NumeraError, Result};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::{Body, Bytes, Frame, Incoming, SizeHint},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::{SessionManager, local::LocalSessionManager},
};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub struct Options {
    pub port: u16,
    pub token: String,
    pub max_body_bytes: usize,
    pub max_connections: usize,
    pub max_sessions: usize,
    pub read_timeout: Duration,
    pub shutdown_timeout: Duration,
}
impl Options {
    pub fn new(port: u16, token: String) -> Self {
        Self {
            port,
            token,
            max_body_bytes: 128 * 1024 * 1024,
            max_connections: 32,
            max_sessions: 64,
            read_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(8),
        }
    }
}

pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=terminate.recv()=>{} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

pub async fn bind(port: u16) -> Result<TcpListener> {
    if port == 0 {
        return Err(NumeraError::new(
            "invalid_input",
            "HTTP port must be from 1 to 65535.",
        ));
    }
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| {
            NumeraError::new(
                "port_unavailable",
                "Local HTTP port is unavailable. Change the port; no alternate port was selected.",
            )
        })
}

type SdkService = StreamableHttpService<Handler, LocalSessionManager>;
type WireBody = BoxBody<Bytes, Infallible>;
struct State {
    options: Options,
    service: SdkService,
    sessions: Arc<LocalSessionManager>,
    initialize: Mutex<()>,
    requests: Arc<Semaphore>,
    stop: CancellationToken,
}
// Hold admission until the complete response stream is consumed or dropped, not just its headers.
struct ResponseBody {
    inner: WireBody,
    _permit: Option<OwnedSemaphorePermit>,
}
impl Body for ResponseBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, Infallible>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
fn response(code: StatusCode, message: &'static str) -> Response<ResponseBody> {
    let mut response = Response::new(ResponseBody {
        inner: Full::new(Bytes::from_static(message.as_bytes())).boxed(),
        _permit: None,
    });
    *response.status_mut() = code;
    response.headers_mut().insert(
        hyper::header::CACHE_CONTROL,
        hyper::header::HeaderValue::from_static("no-store"),
    );
    response
}
fn allowed(request: &Request<Incoming>, state: &State) -> Option<Response<ResponseBody>> {
    let headers = request.headers();
    let authority = format!("127.0.0.1:{}", state.options.port);
    if headers.get_all("host").iter().count() != 1
        || headers.get("host").and_then(|h| h.to_str().ok()) != Some(authority.as_str())
    {
        return Some(response(StatusCode::FORBIDDEN, "Local Host refused."));
    }
    let origins: Vec<_> = headers.get_all("origin").iter().collect();
    let origin = format!("http://{authority}");
    if origins.len() > 1
        || origins
            .first()
            .is_some_and(|v| v.to_str().ok() != Some(origin.as_str()))
    {
        return Some(response(StatusCode::FORBIDDEN, "Local Origin refused."));
    }
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, v)| v);
    if headers.get_all("authorization").iter().count() != 1
        || !auth.is_some_and(|v| bool::from(v.as_bytes().ct_eq(state.options.token.as_bytes())))
    {
        let mut result = response(StatusCode::UNAUTHORIZED, "Local authentication required.");
        result.headers_mut().insert(
            hyper::header::WWW_AUTHENTICATE,
            hyper::header::HeaderValue::from_static("Bearer"),
        );
        return Some(result);
    }
    if request.uri().path() != "/mcp" {
        return Some(response(StatusCode::NOT_FOUND, "Unknown local endpoint."));
    }
    if request.uri().query().is_some() || request.uri().authority().is_some() {
        return Some(response(
            StatusCode::BAD_REQUEST,
            "Only the fixed MCP endpoint is allowed.",
        ));
    }
    if !matches!(
        *request.method(),
        hyper::Method::POST | hyper::Method::GET | hyper::Method::DELETE
    ) {
        return Some(response(
            StatusCode::METHOD_NOT_ALLOWED,
            "Unsupported MCP HTTP method.",
        ));
    }
    None
}
async fn handle(
    request: Request<Incoming>,
    state: Arc<State>,
) -> std::result::Result<Response<ResponseBody>, Infallible> {
    if let Some(response) = allowed(&request, &state) {
        return Ok(response);
    }
    let permit = match state.requests.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(response(
                StatusCode::TOO_MANY_REQUESTS,
                "Local request capacity reached.",
            ));
        }
    };
    let (mut parts, mut body) = request.into_parts();
    if body
        .size_hint()
        .upper()
        .is_some_and(|n| n > state.options.max_body_bytes as u64)
    {
        return Ok(response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "MCP request body exceeds its limit.",
        ));
    }
    let read = async {
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| StatusCode::BAD_REQUEST)?;
            if let Ok(data) = frame.into_data() {
                if bytes
                    .len()
                    .checked_add(data.len())
                    .is_none_or(|n| n > state.options.max_body_bytes)
                {
                    return Err(StatusCode::PAYLOAD_TOO_LARGE);
                }
                bytes.extend_from_slice(&data);
            }
        }
        Ok(bytes)
    };
    let bytes = tokio::select! {
        _=state.stop.cancelled()=>return Ok(response(StatusCode::SERVICE_UNAVAILABLE,"Server is stopping.")),
        result=tokio::time::timeout(state.options.read_timeout,read)=>match result {Ok(Ok(bytes))=>bytes,Ok(Err(code))=>return Ok(response(code,"MCP request body refused.")),Err(_)=>return Ok(response(StatusCode::REQUEST_TIMEOUT,"MCP request body deadline exceeded."))},
    };
    let (bytes, original) = if parts.method == hyper::Method::POST {
        let decoded = match std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| super::js_envelope::decode(text, state.options.max_body_bytes).ok())
        {
            Some(decoded) => decoded,
            None => {
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "Invalid MCP JSON envelope.",
                ));
            }
        };
        let expected_id = decoded.metadata.get("id").cloned();
        let original = decoded.arguments.map(|arguments| {
            let mut original = super::js_envelope::OriginalArguments::new(arguments);
            original.expected_id = expected_id;
            original
        });
        if let Some(original) = &original {
            parts.extensions.insert(original.clone());
        }
        let bytes = match serde_json::to_vec(&decoded.metadata) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "Invalid MCP JSON envelope.",
                ));
            }
        };
        (bytes, original)
    } else {
        (bytes, None)
    };
    let initialization = parts.method == hyper::Method::POST
        && !parts.headers.contains_key("mcp-session-id")
        && serde_json::from_slice::<serde_json::Value>(&bytes)
            .is_ok_and(|v| v["method"] == "initialize");
    // Serialize session creation so concurrent handshakes cannot overshoot the cap.
    let guard = if initialization {
        Some(state.initialize.lock().await)
    } else {
        None
    };
    if initialization && state.sessions.sessions.read().await.len() >= state.options.max_sessions {
        return Ok(response(
            StatusCode::TOO_MANY_REQUESTS,
            "Local session capacity reached.",
        ));
    }
    let wire = Request::from_parts(parts, Full::new(Bytes::from(bytes)));
    let result = state.service.handle(wire).await;
    drop(guard);
    let (mut parts, body) = result.into_parts();
    let successful = parts.status.is_success() && parts.status != StatusCode::ACCEPTED;
    let sse = parts
        .headers
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));
    let body = if successful {
        parts.headers.remove(hyper::header::CONTENT_LENGTH);
        super::js_http_body::JsonBody::new(body, sse, 128 * 1024 * 1024, original).boxed()
    } else {
        body
    };
    Ok(Response::from_parts(
        parts,
        ResponseBody {
            inner: body,
            _permit: Some(permit),
        },
    ))
}

pub async fn serve(
    listener: TcpListener,
    application: Arc<Application>,
    options: Options,
    stop: CancellationToken,
) -> Result<()> {
    if listener
        .local_addr()
        .map_err(|_| NumeraError::runtime())?
        .ip()
        != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        || listener
            .local_addr()
            .map_err(|_| NumeraError::runtime())?
            .port()
            != options.port
        || options.max_connections == 0
        || options.max_sessions == 0
        || options.max_body_bytes == 0
        || options.token.len() < 32
        || options.token.len() > 256
        || !options.token.bytes().all(|b| (0x21..=0x7e).contains(&b))
    {
        return Err(NumeraError::new(
            "invalid_configuration",
            "Invalid local HTTP server options.",
        ));
    }
    let lifetime = Arc::new(super::handler_lifetime::HandlerLifetime::default());
    let handler = Handler::new(application.clone())?.tracked(&lifetime);
    let mut manager = LocalSessionManager::default();
    manager.session_config.init_timeout = Some(Duration::from_secs(8));
    let sessions = Arc::new(manager);
    let sdk_config = StreamableHttpServerConfig::default()
        .with_allowed_hosts([format!("127.0.0.1:{}", options.port)])
        .with_allowed_origins([format!("http://127.0.0.1:{}", options.port)])
        .enforce_origin_validation()
        .with_max_request_body_bytes(options.max_body_bytes)
        .with_cancellation_token(stop.clone());
    let service =
        StreamableHttpService::new(move || Ok(handler.clone()), sessions.clone(), sdk_config);
    let shutdown_timeout = options.shutdown_timeout;
    let read_timeout = options.read_timeout;
    let max_connections = options.max_connections;
    let state = Arc::new(State {
        requests: Arc::new(Semaphore::new(max_connections)),
        options,
        service,
        sessions: sessions.clone(),
        initialize: Mutex::new(()),
        stop: stop.clone(),
    });
    let mut tasks = JoinSet::new();
    application.logger.log(
        "INFO",
        "MCP",
        "Local HTTP server ready.",
        crate::compat::value::json!({"port":state.options.port,"transport":"streamable-http"}),
    );
    let result = loop {
        tokio::select! {
            _=stop.cancelled()=>break Ok(()),
            completed=tasks.join_next(),if !tasks.is_empty()=>{if completed.is_some_and(|r|r.is_err()){break Err(NumeraError::runtime());}},
            accepted=listener.accept()=>{
                let (socket,_)=match accepted {Ok(value)=>value,Err(_)=>break Err(NumeraError::new("network_error","Local HTTP accept failed."))};
                if tasks.len()>=max_connections {drop(socket);continue;}
                let state=state.clone();let close=stop.clone();
                tasks.spawn(async move {
                    let service=service_fn(move |request|handle(request,state.clone()));
                    let mut builder=http1::Builder::new();builder.timer(TokioTimer::new()).header_read_timeout(read_timeout).max_buf_size(65536);
                    let connection=builder.serve_connection(TokioIo::new(socket),service);tokio::pin!(connection);
                    tokio::select! {_=connection.as_mut()=>{},_=close.cancelled()=>{connection.as_mut().graceful_shutdown();let _=tokio::time::timeout(shutdown_timeout,connection.as_mut()).await;}}
                });
            }
        }
    };
    drop(listener);
    stop.cancel();
    application.shutdown();
    let drain = async {
        let ids = sessions
            .sessions
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            sessions
                .close_session(&id)
                .await
                .map_err(|_| NumeraError::runtime())?;
        }
        while let Some(result) = tasks.join_next().await {
            result.map_err(|_| NumeraError::runtime())?;
        }
        Ok::<(), NumeraError>(())
    };
    let cleanup = match tokio::time::timeout(shutdown_timeout, drain).await {
        Ok(Ok(())) => Ok(()),
        outcome => {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            match outcome {
                Ok(Err(error)) => Err(error),
                _ => Err(NumeraError::new(
                    "request_timeout",
                    "Local HTTP shutdown deadline exceeded.",
                )),
            }
        }
    };
    drop(state);
    drop(sessions);
    tokio::time::timeout(shutdown_timeout, lifetime.drained())
        .await
        .map_err(|_| {
            NumeraError::new(
                "request_timeout",
                "Local HTTP handler cleanup deadline exceeded.",
            )
        })?;
    cleanup?;
    application.logger.log(
        "INFO",
        "MCP",
        "Local HTTP server stopped.",
        crate::compat::value::json!({}),
    );
    result
}
