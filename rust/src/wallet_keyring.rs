//! Wallet-derived keyring derivation.
//!
//! Mirrors the frontend's `createUserKeyringFromIkm` /
//! `KeyPair.generatePrivateKey` chain (`pay-interface-app/src/domain/keys/`):
//!
//! 1. Caller signs `sign_message(address)` with the wallet (ed25519 for Solana).
//! 2. The 64-byte signature is the IKM passed to [`derive_keyring_secrets`].
//! 3. HKDF-SHA256(IKM, [`KEYRING_SALT`], "", 32) → seed.
//! 4. For each domain (authority / nullifier / encryption / discovery):
//!    HMAC-SHA256(key=seed, message=domain) → 32-byte secret.
//!
//! Pubkey derivation is left to callers so the crate stays free of a
//! secp256k1 dependency — every secret returned here is a secp256k1 secret
//! except `nullifier`, which is fed straight into SHA-256 to produce the
//! nullifier-key commitment (see [`nullifier_commitment`]).
//!
//! Any external script, CLI, or recovery tool that previously reimplemented
//! HKDF + the four PRF domains by hand can call [`derive_keyring_secrets`]
//! and get bit-identical secrets to the frontend.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// HKDF salt — must match `pay-interface-app/src/app-constants.ts::KEYRING_SALT`.
pub const KEYRING_SALT: &[u8] = b"anoma-pa:keyring-seed";

/// PRF domain separator for the authority signing key (secp256k1 secret).
pub const AUTHORITY_DOMAIN: &[u8] = b"ANOMA_AUTHORITY_KEY";

/// PRF domain separator for the nullifier key (raw 32 bytes; commitment = SHA-256).
pub const NULLIFIER_DOMAIN: &[u8] = b"ANOMA_NULLIFIER_KEY";

/// PRF domain separator for the resource-encryption key (secp256k1 secret).
pub const ENCRYPTION_DOMAIN: &[u8] = b"ANOMA_STATIC_ENCRYPTION_KEY";

/// PRF domain separator for the discovery key (secp256k1 secret).
pub const DISCOVERY_DOMAIN: &[u8] = b"ANOMA_STATIC_DISCOVERY_KEY";

/// Canonical wallet-sign message — must match
/// `pay-interface-app/src/signing-message.ts::getSignMessage`.
///
/// The returned UTF-8 string is what the wallet signs to produce the IKM
/// passed to [`derive_keyring_secrets`]. Any drift from the frontend's
/// exact bytes here will produce a different keyring.
pub fn sign_message(address: &str) -> String {
    format!(
        "I authorize AnomaPay to derive my account from address {address}.\n\
         Do NOT sign this message if the request url is not https://beta.anomapay.app/"
    )
}

/// The four 32-byte secrets that make up a user keyring.
///
/// Three of these (`authority`, `encryption`, `discovery`) are secp256k1
/// secret keys; callers compute their compressed pubkeys with whichever
/// secp256k1 crate they prefer (`k256`, `secp256k1`, `@noble/secp256k1`,
/// the ARM WASM bindings, etc.). The `nullifier` is the raw nk byte string;
/// its commitment is `SHA-256(nullifier)` (see [`nullifier_commitment`]).
#[derive(Clone, Eq, PartialEq)]
pub struct KeyringSecrets {
    pub authority: [u8; 32],
    pub nullifier: [u8; 32],
    pub encryption: [u8; 32],
    pub discovery: [u8; 32],
}

impl std::fmt::Debug for KeyringSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material. Useful for asserting "got a keyring" without leaking.
        f.debug_struct("KeyringSecrets")
            .field("authority", &"<redacted 32 bytes>")
            .field("nullifier", &"<redacted 32 bytes>")
            .field("encryption", &"<redacted 32 bytes>")
            .field("discovery", &"<redacted 32 bytes>")
            .finish()
    }
}

/// Derive the four keyring secrets from a wallet signature over
/// [`sign_message`]`(wallet_address)`.
///
/// `signature_ikm` is treated as opaque key material — for a Solana wallet,
/// this is the 64-byte ed25519 signature bytes.
pub fn derive_keyring_secrets(signature_ikm: &[u8]) -> KeyringSecrets {
    let hk = Hkdf::<Sha256>::new(Some(KEYRING_SALT), signature_ikm);
    let mut seed = [0u8; 32];
    hk.expand(b"", &mut seed)
        .expect("hkdf-sha256 expand to 32 bytes never fails");

    KeyringSecrets {
        authority: prf(&seed, AUTHORITY_DOMAIN),
        nullifier: prf(&seed, NULLIFIER_DOMAIN),
        encryption: prf(&seed, ENCRYPTION_DOMAIN),
        discovery: prf(&seed, DISCOVERY_DOMAIN),
    }
}

/// Nullifier-key commitment used in resources' `nk_commitment` field.
pub fn nullifier_commitment(nk: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(nk);
    hasher.finalize().into()
}

fn prf(seed: &[u8; 32], domain: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(seed).expect("hmac-sha256 accepts any key length");
    mac.update(domain);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture captured from the frontend's `createUserKeyringFromIkm` with
    // this exact IKM. Any drift between Rust and TS implementations is
    // caught here.
    const FIXTURE_IKM: &[u8] = b"frontend-parity-fixture-ikm-32by";

    #[test]
    fn derive_keyring_secrets_matches_frontend_fixture() {
        let secrets = derive_keyring_secrets(FIXTURE_IKM);
        // Independently recomputed via `@noble/hashes` (the frontend's HKDF/HMAC
        // implementation) with the same FIXTURE_IKM, salt, and PRF domains.
        // This is the cross-implementation parity check.
        assert_eq!(
            to_hex(&secrets.authority),
            "942c7cd43cff3dc5419e70a99ff2ab11286ac0c2cba1bb9082079b6881640259"
        );
        assert_eq!(
            to_hex(&secrets.nullifier),
            "dbd1ab0b7369154b26cf2b3bb03d1440239b90f274ae3647366fddaf72d34b10"
        );
        assert_eq!(
            to_hex(&secrets.encryption),
            "d510bfa5078fe90e191687684c62f840c15472c134cadb29e2a747b72789d758"
        );
        assert_eq!(
            to_hex(&secrets.discovery),
            "5fb211e18efda22b42124ca28f80727d9adb65d0240f93bd1859195fc91d0ed4"
        );
    }

    #[test]
    fn deterministic_for_same_ikm() {
        let a = derive_keyring_secrets(FIXTURE_IKM);
        let b = derive_keyring_secrets(FIXTURE_IKM);
        assert_eq!(a.authority, b.authority);
        assert_eq!(a.nullifier, b.nullifier);
        assert_eq!(a.encryption, b.encryption);
        assert_eq!(a.discovery, b.discovery);
    }

    #[test]
    fn different_ikm_produces_different_keys() {
        let a = derive_keyring_secrets(FIXTURE_IKM);
        let b = derive_keyring_secrets(b"a different ikm of any byte length");
        assert_ne!(a.authority, b.authority);
        assert_ne!(a.nullifier, b.nullifier);
    }

    #[test]
    fn nullifier_commitment_is_sha256() {
        let nk = [0xaa_u8; 32];
        let commitment = nullifier_commitment(&nk);
        // sha256(0xaa repeated 32 times) — recomputed in JS for parity.
        assert_eq!(
            to_hex(&commitment),
            "e0e77a507412b120f6ede61f62295b1a7b2ff19d3dcc8f7253e51663470c888e"
        );
    }

    #[test]
    fn sign_message_format_is_stable() {
        assert_eq!(
            sign_message("FoobarWalletAddr111111111111111111111111111"),
            "I authorize AnomaPay to derive my account from address \
             FoobarWalletAddr111111111111111111111111111.\n\
             Do NOT sign this message if the request url is not \
             https://beta.anomapay.app/"
        );
    }

    #[test]
    fn redacted_debug_doesnt_leak_key_material() {
        let secrets = derive_keyring_secrets(FIXTURE_IKM);
        let s = format!("{secrets:?}");
        assert!(s.contains("redacted"));
        assert!(!s.contains(&to_hex(&secrets.authority)));
    }

    fn to_hex(bytes: &[u8]) -> String {
        bytes
            .iter()
            .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            })
    }
}
