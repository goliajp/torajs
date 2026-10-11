use std::string::String;
use std::vec::Vec;

pub(crate) fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex digit"))
        .collect()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| std::format!("{b:02x}")).collect()
}
