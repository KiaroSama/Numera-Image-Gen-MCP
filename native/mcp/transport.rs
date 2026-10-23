use rmcp::{
    RoleServer,
    model::{ClientRequest, ErrorCode, JsonRpcMessage},
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdin, Stdout},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

/// SDK receive currently reads an uncapped line; this boundary never buffers beyond the limit.
pub struct Stdio {
    input: BufReader<Stdin>,
    output: Arc<Mutex<Stdout>>,
    pending: Vec<u8>,
    limit: usize,
    stop: CancellationToken,
    responses: Arc<Mutex<std::collections::HashMap<String, super::js_envelope::OriginalArguments>>>,
}
impl Stdio {
    pub fn new(limit: usize, stop: CancellationToken) -> Self {
        Self {
            input: BufReader::new(tokio::io::stdin()),
            output: Arc::new(Mutex::new(tokio::io::stdout())),
            pending: Vec::new(),
            limit,
            stop,
            responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }
}
impl Transport<RoleServer> for Stdio {
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let output = self.output.clone();
        let responses = self.responses.clone();
        async move {
            let metadata = serde_json::to_string(&item)
                .map_err(|_| io::Error::other("Cannot encode protocol response"))?;
            let mut message = crate::compat::js_json::parse_bounded(&metadata, 128 * 1024 * 1024)
                .map_err(|_| io::Error::other("Cannot encode protocol response"))?;
            let key = message
                .get("id")
                .map(crate::compat::js_json::stringify)
                .transpose()
                .map_err(|_| io::Error::other("Cannot encode protocol identifier"))?;
            let response = if let Some(key) = key {
                responses.lock().await.remove(&key)
            } else {
                None
            };
            if let Some(response) = response {
                super::js_envelope::restore_result(&mut message, &response)
                    .map_err(|_| io::Error::other("Cannot restore protocol result"))?;
            }
            super::js_envelope::restore_id(&mut message)
                .map_err(|_| io::Error::other("Cannot restore protocol identifier"))?;
            let mut bytes = crate::compat::js_json::stringify(&message)
                .map_err(|_| io::Error::other("Cannot encode protocol response"))?
                .into_bytes();
            bytes.push(b'\n');
            tokio::time::timeout(Duration::from_secs(30), async {
                let mut output = output.lock().await;
                output.write_all(&bytes).await?;
                output.flush().await
            })
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "Protocol output deadline exceeded")
            })?
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            let available = tokio::select! {_=self.stop.cancelled()=>return None,result=self.input.fill_buf()=>match result{Ok(bytes)=>bytes,Err(_)=>{self.stop.cancel();return None;}}};
            if available.is_empty() {
                self.stop.cancel();
                return None;
            }
            let count = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |n| n + 1);
            if self
                .pending
                .len()
                .checked_add(count)
                .is_none_or(|n| n > self.limit)
            {
                self.stop.cancel();
                return None;
            }
            self.pending.extend_from_slice(&available[..count]);
            let complete = available[count - 1] == b'\n';
            self.input.consume(count);
            if !complete {
                continue;
            }
            let parsed = std::str::from_utf8(&self.pending)
                .map_err(|_| ())
                .and_then(|text| super::js_envelope::decode(text, self.limit).map_err(|_| ()))
                .and_then(|decoded| {
                    use rmcp::model::GetExtensions;
                    let mut message: RxJsonRpcMessage<RoleServer> =
                        serde_json::from_value(decoded.metadata).map_err(|_| ())?;
                    let mut registration = None;
                    if let Some(arguments) = decoded.arguments
                        && let rmcp::model::JsonRpcMessage::Request(request) = &mut message
                    {
                        let mut original = super::js_envelope::OriginalArguments::new(arguments);
                        original.expected_id =
                            Some(serde_json::to_value(&request.id).map_err(|_| ())?);
                        registration = Some((
                            serde_json::to_string(&request.id).map_err(|_| ())?,
                            original.clone(),
                        ));
                        request.request.extensions_mut().insert(original);
                    }
                    Ok((message, registration))
                });
            self.pending.clear();
            match parsed {
                Ok((message, registration)) => {
                    // The frozen source has no modern bootstrap handler. Let the client fall back
                    // to legacy initialization instead of RMCP rejecting its inline version first.
                    if let JsonRpcMessage::Request(request) = &message
                        && matches!(&request.request, ClientRequest::DiscoverRequest(_))
                    {
                        let error = JsonRpcMessage::error(
                            rmcp::ErrorData::new(
                                ErrorCode::METHOD_NOT_FOUND,
                                "Method not found",
                                None,
                            ),
                            Some(request.id.clone()),
                        );
                        if self.send(error).await.is_err() {
                            self.stop.cancel();
                            return None;
                        }
                        continue;
                    }
                    if let Some((key, original)) = registration {
                        let mut responses = self.responses.lock().await;
                        if responses.len() >= 128 || responses.contains_key(&key) {
                            self.stop.cancel();
                            return None;
                        }
                        responses.insert(key, original);
                    }
                    return Some(message);
                }
                Err(_) => {
                    self.stop.cancel();
                    return None;
                }
            }
        }
    }
    async fn close(&mut self) -> io::Result<()> {
        self.stop.cancel();
        self.pending.clear();
        self.responses.lock().await.clear();
        tokio::time::timeout(Duration::from_secs(5), self.output.lock().await.shutdown())
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Protocol shutdown deadline exceeded",
                )
            })?
    }
}
