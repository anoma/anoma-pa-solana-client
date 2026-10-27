//! Generates, from `idls/protocol_adapter.json`'s errors, the conversion of a
//! custom program error code to the error enum `declare_program!` generates
//! from the same IDL, which neither it nor Anchor provides.

use std::fmt::Write as _;
use std::path::Path;

const IDL: &str = "idls/protocol_adapter.json";

fn main() {
    println!("cargo:rerun-if-changed={IDL}");
    println!("cargo:rerun-if-changed=build.rs");

    let text =
        std::fs::read_to_string(IDL).unwrap_or_else(|e| panic!("read {IDL} for its errors: {e}"));
    let idl: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{IDL} is not JSON: {e}"));

    // `declare_program!` names the enum after the program, in upper camel
    // case, then `Error`, and each variant by its IDL name.
    let program = idl["metadata"]["name"]
        .as_str()
        .unwrap_or_else(|| panic!("{IDL} has no metadata.name"));
    let mut enum_name: String = program
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect();
    enum_name.push_str("Error");

    let errors = idl["errors"]
        .as_array()
        .unwrap_or_else(|| panic!("{IDL} has no errors list"));
    let mut arms = String::new();
    for error in errors {
        let code = error["code"]
            .as_u64()
            .unwrap_or_else(|| panic!("{IDL}: an error without a code: {error}"));
        let name = error["name"]
            .as_str()
            .unwrap_or_else(|| panic!("{IDL}: an error without a name: {error}"));
        writeln!(arms, "            {code} => Ok({enum_name}::{name}),").unwrap();
    }

    let source = format!(
        "/// The error `{program}` returns as custom program error `code`, or the
/// code when its IDL names no such error.
impl TryFrom<u32> for {enum_name} {{
    type Error = u32;

    fn try_from(code: u32) -> Result<Self, u32> {{
        match code {{
{arms}            _ => Err(code),
        }}
    }}
}}
"
    );
    let out = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("protocol_adapter_error_codes.rs");
    std::fs::write(&out, source).unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
}
