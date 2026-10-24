//! The key, encrypted in a file, for when the system has no secret store. Nothing to type: a
//! random 256-bit key, kept in its own file, seals it with XChaCha20-Poly1305.
//!
//! File: magic (format version) | nonce (24) | ciphertext and tag. The magic is authenticated
//! too, so a changed header fails like a changed byte anywhere else.

use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};

const MAGIC: &[u8; 6] = b"ASTRO\x02";
const NONCE: usize = 24;

/// A new random key.
pub fn new_master() -> [u8; 32] {
    Key::generate().into()
}

pub fn seal(master: &[u8; 32], secret: &str) -> Result<Vec<u8>, String> {
    let nonce = XNonce::generate();
    let sealed = XChaCha20Poly1305::new(master.into()).encrypt(&nonce, Payload { msg: secret.as_bytes(), aad: MAGIC }).map_err(|e| e.to_string())?;
    Ok([MAGIC.as_slice(), nonce.as_slice(), &sealed].concat())
}

/// `None` when the file is not one this app wrote with this key, or was changed.
pub fn open(master: &[u8; 32], file: &[u8]) -> Option<String> {
    if file.len() <= MAGIC.len() + NONCE || &file[..MAGIC.len()] != MAGIC {
        return None;
    }
    let (nonce, sealed) = file[MAGIC.len()..].split_at(NONCE);
    let nonce = XNonce::try_from(nonce).ok()?;
    let bytes = XChaCha20Poly1305::new(master.into()).decrypt(&nonce, Payload { msg: sealed, aad: MAGIC }).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "GCDC_abc.def+/=é";

    #[test]
    fn opens_what_it_sealed() {
        let master = new_master();
        let file = seal(&master, KEY).unwrap();
        assert_eq!(open(&master, &file).as_deref(), Some(KEY));
        // Two seals of the same secret differ, and the secret is not in the clear.
        assert_ne!(file, seal(&master, KEY).unwrap());
        assert!(!file.windows(KEY.len()).any(|w| w == KEY.as_bytes()));
        assert_ne!(master, new_master());
        // Empty and very long secrets.
        assert_eq!(open(&master, &seal(&master, "").unwrap()).as_deref(), Some(""));
        assert_eq!(open(&master, &seal(&master, &"k".repeat(5000)).unwrap()).map(|s| s.len()), Some(5000));
    }

    #[test]
    fn refuses_another_key_and_any_change() {
        let master = new_master();
        let file = seal(&master, KEY).unwrap();
        assert_eq!(open(&new_master(), &file), None);
        // Every byte, the magic included: flipping it fails, never returns another secret.
        for i in 0..file.len() {
            let mut bad = file.clone();
            bad[i] ^= 1;
            assert_eq!(open(&master, &bad), None, "byte {i}");
        }
        // Cut short, empty, or not a vault at all.
        for bad in [&[][..], b"ASTRO\x02", &file[..file.len() - 1], &file[..MAGIC.len() + NONCE], b"not a vault at all, but long enough to have a head: ......"] {
            assert_eq!(open(&master, bad), None, "{} bytes", bad.len());
        }
    }
}
