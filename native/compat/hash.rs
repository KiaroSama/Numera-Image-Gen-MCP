use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in hash {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 15) as usize] as char);
    }
    result
}
