//! The bindings Anchor's `declare_program!` generates from the IDLs committed
//! in `idls/`: the adapter's (`protocol_adapter`: its id, accounts, events,
//! types, constants, errors, and the instruction accounts and arguments of
//! `client`) and the verifier router's (`verifier_router`, whose
//! `VerifierEntry` names the verifier a settlement passes).

/// What `declare_program!` reads as `anchor_lang`: the real crate, with the
/// IDL's `u256` added to its prelude. Anchor's generator writes the IDL type
/// `u256` as a Rust type of that name, which Rust lacks
/// (otter-sec/anchor#5224); on the wire it is 32 little-endian bytes.
#[doc(hidden)]
pub mod anchor_lang {
    pub use ::anchor::*;

    pub mod prelude {
        pub use super::super::U256LeBytes as u256;
        pub use ::anchor::prelude::*;
        // Anchor's prelude names the real crate `anchor_lang`, as the module
        // `declare_program!` reads it from does; this names the shim the
        // same, so the two do not conflict.
        pub use super::super::anchor_lang;
    }
}

/// A `u256` as Borsh writes it: its 32 little-endian bytes. A type of this
/// crate, not an alias of `[u8; 32]`: under `idl-build`, the `IdlBuild` derive
/// on every generated type with a `u256` field calls `IdlBuild` on `u256`,
/// which this crate cannot implement for `[u8; 32]`.
#[derive(
    anchor_lang::AnchorSerialize, anchor_lang::AnchorDeserialize, Clone, Copy, Debug, Default,
)]
pub struct U256LeBytes(pub [u8; 32]);

::anchor::declare_program!(protocol_adapter);
::anchor::declare_program!(verifier_router);

use protocol_adapter::error::ProtocolAdapterError;

// `impl TryFrom<u32> for ProtocolAdapterError`, from the IDL's errors
// (`build.rs`).
include!(concat!(env!("OUT_DIR"), "/protocol_adapter_error_codes.rs"));

/// Two errors are equal when their codes are.
impl PartialEq for ProtocolAdapterError {
    fn eq(&self, other: &Self) -> bool {
        u32::from(*self) == u32::from(*other)
    }
}

impl Eq for ProtocolAdapterError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn idl_errors() -> Vec<serde_json::Value> {
        let idl: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/idls/protocol_adapter.json"
        )))
        .unwrap();
        idl["errors"].as_array().unwrap().clone()
    }

    #[test]
    fn every_idl_error_code_is_its_error() {
        let errors = idl_errors();
        assert!(!errors.is_empty(), "the IDL declares errors");
        for error in &errors {
            let code = u32::try_from(error["code"].as_u64().unwrap()).unwrap();
            let decoded = ProtocolAdapterError::try_from(code)
                .unwrap_or_else(|code| panic!("{code}, the IDL's {error}, does not convert"));
            println!("{code}: {} ({decoded})", decoded.name());
            assert_eq!(decoded.name(), error["name"].as_str().unwrap(), "{code}");
            assert_eq!(
                decoded.to_string(),
                error["msg"].as_str().unwrap(),
                "{code}"
            );
            assert_eq!(u32::from(decoded), code);
        }
    }

    #[test]
    fn a_code_outside_the_idls_errors_is_err() {
        let codes: Vec<u64> = idl_errors()
            .iter()
            .map(|e| e["code"].as_u64().unwrap())
            .collect();
        let first = u32::try_from(*codes.iter().min().unwrap()).unwrap();
        let past = u32::try_from(*codes.iter().max().unwrap()).unwrap() + 1;
        for code in [0, 2006, first - 1, past, u32::MAX] {
            assert_eq!(
                ProtocolAdapterError::try_from(code).map(|e| e.name()),
                Err(code),
                "{code}"
            );
        }
    }
}
