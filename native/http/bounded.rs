use crate::error::{NumeraError, Result};

pub struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}

impl BoundedBytes {
    pub fn new(advertised: Option<u64>, present: bool, limit: usize) -> Result<Self> {
        if advertised.is_some_and(|length| length > limit as u64) {
            return Err(byte_limit());
        }
        if !present {
            return Err(
                NumeraError::new("invalid_response", "Response body is empty.").stage("response"),
            );
        }
        Ok(Self {
            bytes: Vec::new(),
            limit,
        })
    }

    pub fn push(&mut self, chunk: &[u8], cancellation: Option<NumeraError>) -> Result<()> {
        if let Some(reason) = cancellation {
            return Err(reason);
        }
        if self
            .bytes
            .len()
            .checked_add(chunk.len())
            .is_none_or(|total| total > self.limit)
        {
            return Err(byte_limit());
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

fn byte_limit() -> NumeraError {
    NumeraError::new("invalid_response", "Response exceeds byte limit.").stage("response")
}
