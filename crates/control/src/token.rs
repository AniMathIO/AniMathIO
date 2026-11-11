//! Random bearer-token generation, `0600` storage and constant-time compare.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use subtle::ConstantTimeEq;

/// Number of random bytes in a control token (256 bits).
const TOKEN_BYTES: usize = 32;

/// Generate a fresh hex-encoded token.
pub(crate) fn generate() -> io::Result<String> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(format!("token generation failed: {error}")))?;
    Ok(hex(&bytes))
}

/// Write the token to a file readable only by the current user.
pub(crate) fn write_file(path: &Path, token: &str) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(token.as_bytes())?;
    file.flush()
}

/// Read a token file, trimming surrounding whitespace.
pub(crate) fn read_file(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map(|value| value.trim().to_owned())
}

/// Compare a provided token against the expected one in constant time.
pub(crate) fn matches(provided: Option<&str>, expected: &str) -> bool {
    let Some(provided) = provided else {
        return false;
    };
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    provided.len() == expected.len() && bool::from(provided.ct_eq(expected))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_unique_and_hex() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_eq!(a.len(), TOKEN_BYTES * 2);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn comparison_is_exact_and_case_sensitive() {
        let token = generate().unwrap();
        assert!(matches(Some(&token), &token));
        assert!(!matches(None, &token));
        assert!(!matches(Some(""), &token));
        assert!(!matches(Some(&token.to_uppercase()), &token));
        assert!(!matches(Some(&token[..token.len() - 2]), &token));
    }
}
