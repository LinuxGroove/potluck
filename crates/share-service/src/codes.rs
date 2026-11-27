//! Share codes, tokens and the signing key.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};

/// The game server's share code letters (game-server `Util.CODE_ALPHABET`):
/// no 0, 1, I, L, O or U, so a code read aloud can't be misheard.
pub const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const LENGTH: usize = 8;

fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("the system's random numbers");
    b
}

pub fn new_code() -> String {
    let mut out = String::new();
    while out.len() < LENGTH {
        for byte in random_bytes(16) {
            // 240 is the largest multiple of 30 below 256: no letter comes up more often
            if byte < 240 && out.len() < LENGTH {
                out.push(ALPHABET[(byte % 30) as usize] as char);
            }
        }
    }
    out
}

/// What a player typed, as a code: case, spaces and dashes don't matter.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

pub fn valid(code: &str) -> bool {
    code.len() == LENGTH && code.bytes().all(|b| ALPHABET.contains(&b))
}

/// `N8GT3WFA` as `N8GT-3WFA`, the way the game shows it.
pub fn pretty(code: &str) -> String {
    if code.len() == LENGTH { format!("{}-{}", &code[..4], &code[4..]) } else { code.to_string() }
}

/// A long random token for sessions and one-time links.
pub fn token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(24))
}

/// The key manifests are signed with, made on the first run and kept in the
/// data folder. Games check manifests with its public half.
pub fn signing_key(data: &std::path::Path) -> std::io::Result<SigningKey> {
    let path = data.join("signing.key");
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(text.trim()) {
            if let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice()) {
                return Ok(SigningKey::from_bytes(&seed));
            }
        }
    }
    let seed: [u8; 32] = random_bytes(32).try_into().expect("32 bytes");
    let key = SigningKey::from_bytes(&seed);
    std::fs::write(&path, base64::engine::general_purpose::STANDARD.encode(seed))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

pub fn sign(key: &SigningKey, text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.sign(text.as_bytes()).to_bytes())
}

/// Checks a manifest's signature against the market's public key, as the
/// game does before it runs anything a code brought.
pub fn verify(public: &str, text: &str, signature: &str) -> bool {
    let b64 = base64::engine::general_purpose::STANDARD;
    let (Ok(k), Ok(sig)) = (b64.decode(public), b64.decode(signature)) else { return false };
    let (Ok(k), Ok(sig)) = (<[u8; 32]>::try_from(k.as_slice()), <[u8; 64]>::try_from(sig.as_slice())) else { return false };
    let Ok(k) = ed25519_dalek::VerifyingKey::from_bytes(&k) else { return false };
    k.verify_strict(text.as_bytes(), &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
}

pub fn public_key(key: &SigningKey) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.verifying_key().to_bytes())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(bytes))
}

/// Git's name for a file's contents: `blob <len>\0` then the bytes, in SHA-1.
pub fn git_blob_sha(bytes: &[u8]) -> String {
    use sha1::Digest;
    let mut h = sha1::Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
