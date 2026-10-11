//! Project Wycheproof vectors for ChaCha20-Poly1305 (tests/data,
//! Apache-2.0).
//!
//! The runner counts the tests against `numberOfTests`, so a parser that
//! silently skips a group fails.

mod json;

use json::{Json, unhex};
use torajs_crypto::Aead;
use torajs_crypto::aead::{AeadError, TAG_LEN};
use torajs_crypto::chacha20poly1305::ChaCha20Poly1305;

fn load(name: &str) -> Json {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    Json::parse(&std::fs::read_to_string(&path).expect("read vector file"))
}

struct Vector {
    id: f64,
    key: Vec<u8>,
    iv: Vec<u8>,
    aad: Vec<u8>,
    msg: Vec<u8>,
    ct: Vec<u8>,
    tag: Vec<u8>,
    valid: bool,
}

/// Calls `run` for every test of `file` and checks the count.
fn each(file: &str, mut run: impl FnMut(&Vector)) {
    let doc = load(file);
    let mut ran = 0;
    for group in doc.get("testGroups").arr() {
        assert_eq!(group.get("tagSize").num(), 128.0, "{file}: tag size");
        for t in group.get("tests").arr() {
            let id = t.get("tcId").num();
            let v = Vector {
                id,
                key: unhex(t.get("key").str()),
                iv: unhex(t.get("iv").str()),
                aad: unhex(t.get("aad").str()),
                msg: unhex(t.get("msg").str()),
                ct: unhex(t.get("ct").str()),
                tag: unhex(t.get("tag").str()),
                valid: match t.get("result").str() {
                    "valid" => true,
                    "invalid" => false,
                    other => panic!("{file} #{id}: unexpected result {other}"),
                },
            };
            run(&v);
            ran += 1;
        }
    }
    assert_eq!(ran, doc.get("numberOfTests").num() as usize, "{file}");
}

/// Checks one vector against a seal and an open function.
fn check(
    v: &Vector,
    seal: impl Fn(&mut [u8]) -> Result<[u8; TAG_LEN], AeadError>,
    open: impl Fn(&mut [u8], &[u8; TAG_LEN]) -> Result<(), AeadError>,
) {
    let id = v.id;
    let Ok(want_tag) = <[u8; TAG_LEN]>::try_from(&v.tag[..]) else {
        // a tag of another length cannot be passed to `open`
        assert!(
            !v.valid,
            "#{id}: valid vector with a {}-byte tag",
            v.tag.len()
        );
        return;
    };
    let mut buf = v.ct.clone();
    let opened = open(&mut buf, &want_tag);
    if v.valid {
        assert_eq!(opened, Ok(()), "#{id}: open");
        assert_eq!(buf, v.msg, "#{id}: plaintext");
        let mut buf = v.msg.clone();
        let tag = seal(&mut buf).expect("seal");
        assert_eq!(buf, v.ct, "#{id}: ciphertext");
        assert_eq!(tag, want_tag, "#{id}: tag");
    } else {
        assert_eq!(opened, Err(AeadError), "#{id}: invalid vector accepted");
        assert!(buf.iter().all(|&b| b == 0), "#{id}: buffer not wiped");
    }
}

#[test]
fn chacha20_poly1305() {
    let mut wrong_nonce = 0;
    each("chacha20_poly1305_test.json", |v| {
        let key = ChaCha20Poly1305::new(v.key[..].try_into().unwrap());
        let Ok(nonce) = <&[u8; 12]>::try_from(&v.iv[..]) else {
            // the API only takes 96-bit nonces, so these cannot be passed
            assert!(
                !v.valid,
                "#{}: valid vector with a {}-byte nonce",
                v.id,
                v.iv.len()
            );
            wrong_nonce += 1;
            return;
        };
        check(
            v,
            |b| key.seal(nonce, &v.aad, b),
            |b, t| key.open(nonce, &v.aad, b, t),
        );
    });
    assert_eq!(wrong_nonce, 9);
}
