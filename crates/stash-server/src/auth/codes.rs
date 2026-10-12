//! Random secrets, human-typable codes and hashing (section 7).
//!
//! Every secret is generated from the thread-local CSPRNG and stored only as a
//! SHA-256 hex digest. Plaintext leaves the server once: in the response or
//! log line that hands it to a person or device.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Prefix of every session and device token.
pub const TOKEN_PREFIX: &str = "pst_";

/// 32 symbols without look-alikes (no 0/O, no 1/I): 5 bits per character.
pub const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Fill `N` bytes from the CSPRNG.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::rng().fill_bytes(&mut buf);
    buf
}

/// Unpadded base64url of `N` random bytes.
pub fn random_b64<const N: usize>() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<N>())
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// A new session or device token: `pst_` + 43 base64url characters (256 bits).
pub fn new_token() -> String {
    format!("{TOKEN_PREFIX}{}", random_b64::<32>())
}

/// The 8-character display prefix of a token (first 8 characters after `pst_`).
pub fn token_display_prefix(token: &str) -> String {
    token.strip_prefix(TOKEN_PREFIX).unwrap_or(token).chars().take(8).collect()
}

/// Groups of 4 characters from [`CODE_ALPHABET`], joined with `-`.
pub fn grouped_code(groups: usize) -> String {
    let bytes = random_bytes::<64>();
    let mut out = String::with_capacity(groups * 5);
    for (i, b) in bytes.iter().take(groups * 4).enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        // 256 is a multiple of 32, so masking keeps the distribution uniform.
        out.push(CODE_ALPHABET[(b & 31) as usize] as char);
    }
    out
}

/// First-run setup code: `XXXX-XXXX-XXXX` (60 bits).
pub fn new_setup_code() -> String {
    grouped_code(3)
}

/// Pairing user code: `ABCD-EFGH` (40 bits, short-lived and rate limited).
pub fn new_user_code() -> String {
    grouped_code(2)
}

/// Normalize a typed code: drop spaces and dashes, uppercase, regroup by 4.
/// Returns None when the result contains characters outside the alphabet or
/// does not have exactly `groups` groups.
pub fn normalize_code(input: &str, groups: usize) -> Option<String> {
    let chars: Vec<char> = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if chars.len() != groups * 4 || !chars.iter().all(|c| c.is_ascii() && CODE_ALPHABET.contains(&(*c as u8))) {
        return None;
    }
    let mut out = String::with_capacity(groups * 5);
    for (i, c) in chars.into_iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(c);
    }
    Some(out)
}

/// Compare two strings without an early exit on the first difference.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_format() {
        let t = new_token();
        assert!(t.starts_with("pst_"));
        assert_eq!(t.len(), 4 + 43);
        assert!(t[4..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(new_token(), t);
        assert_eq!(token_display_prefix(&t), t[4..12]);
        assert_eq!(token_display_prefix(&t).len(), 8);
    }

    #[test]
    fn invite_code_is_128_bits() {
        let code = random_b64::<16>();
        assert_eq!(code.len(), 22);
        assert_eq!(URL_SAFE_NO_PAD.decode(&code).unwrap().len(), 16);
    }

    #[test]
    fn alphabet_is_unambiguous_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in CODE_ALPHABET {
            assert!(seen.insert(*c), "duplicate {}", *c as char);
            assert!(!b"01OI".contains(c));
        }
        assert_eq!(seen.len(), 32);
    }

    #[test]
    fn setup_and_user_code_format() {
        let setup = new_setup_code();
        assert_eq!(setup.len(), 14);
        let parts: Vec<&str> = setup.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.len() == 4 && p.bytes().all(|b| CODE_ALPHABET.contains(&b))));
        let user = new_user_code();
        assert_eq!(user.len(), 9);
        assert_eq!(user.as_bytes()[4], b'-');
    }

    #[test]
    fn normalize_typed_codes() {
        assert_eq!(normalize_code("abcd-efgh", 2).as_deref(), Some("ABCD-EFGH"));
        assert_eq!(normalize_code(" ab cd efgh ", 2).as_deref(), Some("ABCD-EFGH"));
        assert_eq!(normalize_code("ABCDEFGHJKMN", 3).as_deref(), Some("ABCD-EFGH-JKMN"));
        assert_eq!(normalize_code("ABCD-EFG", 2), None);
        assert_eq!(normalize_code("ABCD-EFG0", 2), None);
        assert_eq!(normalize_code("ABCD-EFGHI", 2), None);
        let code = new_setup_code();
        assert_eq!(normalize_code(&code.to_lowercase(), 3), Some(code));
    }

    #[test]
    fn hashing() {
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex("x").len(), 64);
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
