//! Project Wycheproof vectors for HMAC and HKDF (tests/data, Apache-2.0).

mod json;

use json::{Json, unhex};
use torajs_crypto::Digest;
use torajs_crypto::hkdf::Hkdf;
use torajs_crypto::hmac::Hmac;
use torajs_crypto::sha2::{Sha256, Sha384, Sha512};

fn load(name: &str) -> Json {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    Json::parse(&std::fs::read_to_string(&path).expect("read vector file"))
}

// both runners count the tests they ran against `numberOfTests`, so a
// parser that silently skips a group fails
fn hmac_file<D: Digest>(file: &str) {
    let doc = load(file);
    let mut ran = 0;
    for group in doc.get("testGroups").arr() {
        let tag_bits = group.get("tagSize").num() as usize;
        for t in group.get("tests").arr() {
            let id = t.get("tcId").num();
            let key = unhex(t.get("key").str());
            let msg = unhex(t.get("msg").str());
            let tag = unhex(t.get("tag").str());
            assert_eq!(tag.len() * 8, tag_bits, "{file} #{id}: tag size");
            let mac = Hmac::<D>::new(&key).chain(&msg);
            let ok = if tag.len() == D::OUTPUT_LEN {
                mac.verify(&tag).is_ok()
            } else {
                mac.verify_truncated(&tag).is_ok()
            };
            let want = match t.get("result").str() {
                "valid" => true,
                "invalid" => false,
                other => panic!("{file} #{id}: unexpected result {other}"),
            };
            assert_eq!(ok, want, "{file} #{id}");
            ran += 1;
        }
    }
    assert_eq!(ran, doc.get("numberOfTests").num() as usize, "{file}");
}

fn hkdf_file<D: Digest>(file: &str) {
    let doc = load(file);
    let mut ran = 0;
    for group in doc.get("testGroups").arr() {
        for t in group.get("tests").arr() {
            let id = t.get("tcId").num();
            let ikm = unhex(t.get("ikm").str());
            let salt = unhex(t.get("salt").str());
            let info = unhex(t.get("info").str());
            let size = t.get("size").num() as usize;
            let mut okm = vec![0u8; size];
            let got = Hkdf::<D>::extract(&salt, &ikm).expand(&info, &mut okm);
            match t.get("result").str() {
                "valid" => {
                    assert!(got.is_ok(), "{file} #{id}");
                    assert_eq!(okm, unhex(t.get("okm").str()), "{file} #{id}");
                }
                "invalid" => assert!(got.is_err(), "{file} #{id}"),
                other => panic!("{file} #{id}: unexpected result {other}"),
            }
            ran += 1;
        }
    }
    assert_eq!(ran, doc.get("numberOfTests").num() as usize, "{file}");
}

#[test]
fn hmac_sha256() {
    hmac_file::<Sha256>("hmac_sha256_test.json");
}

#[test]
fn hmac_sha384() {
    hmac_file::<Sha384>("hmac_sha384_test.json");
}

#[test]
fn hmac_sha512() {
    hmac_file::<Sha512>("hmac_sha512_test.json");
}

#[test]
fn hkdf_sha256() {
    hkdf_file::<Sha256>("hkdf_sha256_test.json");
}

#[test]
fn hkdf_sha384() {
    hkdf_file::<Sha384>("hkdf_sha384_test.json");
}

#[test]
fn hkdf_sha512() {
    hkdf_file::<Sha512>("hkdf_sha512_test.json");
}
