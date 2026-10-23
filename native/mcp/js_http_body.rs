use super::js_envelope::{OriginalArguments, restore_id, restore_result};
use crate::compat::js_json;
use http_body_util::combinators::BoxBody;
use hyper::body::{Body, Bytes, Frame, SizeHint};
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

/// SDK protocol/SSE semantics remain unchanged; only exact IDs and this request's typed result
/// are restored. No user payload markers or whole-connection SSE buffering are used.
pub struct JsonBody {
    inner: BoxBody<Bytes, Infallible>,
    original: Option<OriginalArguments>,
    pending: Vec<u8>,
    limit: usize,
    sse: bool,
    done: bool,
}
impl JsonBody {
    pub fn new(
        inner: BoxBody<Bytes, Infallible>,
        sse: bool,
        limit: usize,
        original: Option<OriginalArguments>,
    ) -> Self {
        Self {
            inner,
            original,
            pending: Vec::new(),
            limit,
            sse,
            done: false,
        }
    }
    fn json(&self, bytes: &[u8]) -> Vec<u8> {
        let encoded = (|| {
            let text = std::str::from_utf8(bytes).ok()?;
            let mut value = js_json::parse_bounded(text, self.limit).ok()?;
            if let Some(original) = &self.original {
                restore_result(&mut value, original).ok()?;
            }
            restore_id(&mut value).ok()?;
            js_json::stringify(&value).ok().map(String::into_bytes)
        })();
        encoded.unwrap_or_else(|| b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32603,\"message\":\"Cannot encode the protocol response.\"}}".to_vec())
    }
    fn lines(&mut self) -> Option<Vec<u8>> {
        let end = self.pending.iter().rposition(|b| *b == b'\n')? + 1;
        let mut output = Vec::new();
        for line in self.pending[..end].split_inclusive(|b| *b == b'\n') {
            if let Some(data) = line.strip_prefix(b"data:")
                && data.iter().any(|b| !b.is_ascii_whitespace())
            {
                output.extend_from_slice(b"data: ");
                output.extend(self.json(data.strip_prefix(b" ").unwrap_or(data)));
                output.push(b'\n');
            } else {
                output.extend_from_slice(line);
            }
        }
        self.pending.drain(..end);
        Some(output)
    }
}
impl Body for JsonBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.done {
            return Poll::Ready(None);
        }
        let mut consumed = 0;
        loop {
            if consumed >= 32 {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            consumed += 1;
            match Pin::new(&mut self.inner).poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        if self
                            .pending
                            .len()
                            .checked_add(data.len())
                            .is_none_or(|n| n > self.limit)
                        {
                            self.done = true;
                            let error = self.json(b"invalid");
                            let bytes = if self.sse {
                                [b"data: ".as_slice(), error.as_slice(), b"\n\n".as_slice()]
                                    .concat()
                            } else {
                                error
                            };
                            return Poll::Ready(Some(Ok(Frame::data(Bytes::from(bytes)))));
                        }
                        self.pending.extend_from_slice(&data);
                        if self.sse
                            && self.pending.contains(&b'\n')
                            && let Some(bytes) = self.lines()
                        {
                            return Poll::Ready(Some(Ok(Frame::data(Bytes::from(bytes)))));
                        }
                    }
                }
                Poll::Ready(None) => {
                    self.done = true;
                    if self.pending.is_empty() {
                        return Poll::Ready(None);
                    }
                    let bytes = if self.sse {
                        [
                            b"data: ".as_slice(),
                            self.json(b"invalid").as_slice(),
                            b"\n\n".as_slice(),
                        ]
                        .concat()
                    } else {
                        self.json(&self.pending)
                    };
                    self.pending.clear();
                    return Poll::Ready(Some(Ok(Frame::data(Bytes::from(bytes)))));
                }
                Poll::Ready(Some(Err(never))) => match never {},
            }
        }
    }
    fn is_end_stream(&self) -> bool {
        self.done
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}
