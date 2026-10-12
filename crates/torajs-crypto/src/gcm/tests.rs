use std::vec::Vec;

use super::key::Core;
use super::*;
use crate::aead::{AeadError, TAG_LEN};
use crate::cpu::CpuFeatures;
use crate::testutil::{hex, unhex};

struct Case {
    n: u32,
    key: &'static str,
    iv: &'static str,
    pt: &'static str,
    aad: &'static str,
    ct: &'static str,
    tag: &'static str,
}

const P4: &str = "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39";
const A4: &str = "feedfacedeadbeeffeedfacedeadbeefabaddad2";

/// Test cases from McGrew and Viega, "The Galois/Counter Mode of
/// Operation", the examples NIST publishes for SP 800-38D.
const CASES: &[Case] = &[
    Case {
        n: 1,
        key: "00000000000000000000000000000000",
        iv: "000000000000000000000000",
        pt: "",
        aad: "",
        ct: "",
        tag: "58e2fccefa7e3061367f1d57a4e7455a",
    },
    Case {
        n: 2,
        key: "00000000000000000000000000000000",
        iv: "000000000000000000000000",
        pt: "00000000000000000000000000000000",
        aad: "",
        ct: "0388dace60b6a392f328c2b971b2fe78",
        tag: "ab6e47d42cec13bdf53a67b21257bddf",
    },
    Case {
        n: 4,
        key: "feffe9928665731c6d6a8f9467308308",
        iv: "cafebabefacedbaddecaf888",
        pt: P4,
        aad: A4,
        ct: "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091",
        tag: "5bc94fbc3221a5db94fae95ae7121a47",
    },
    Case {
        n: 6,
        key: "feffe9928665731c6d6a8f9467308308",
        iv: "9313225df88406e555909c5aff5269aa6a7a9538534f7da1e4c303d2a318a728c3c0c95156809539fcf0e2429a6b525416aedbf5a0de6a57a637b39b",
        pt: P4,
        aad: A4,
        ct: "8ce24998625615b603a033aca13fb894be9112a5c3a211a8ba262a3cca7e2ca701e4a9a4fba43c90ccdcb281d48c7c6fd62875d2aca417034c34aee5",
        tag: "619cc5aefffe0bfa462af43c1699d050",
    },
    Case {
        n: 16,
        key: "feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308",
        iv: "cafebabefacedbaddecaf888",
        pt: P4,
        aad: A4,
        ct: "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
        tag: "76fc6ece0f4e1768cddf8853bb2d551b",
    },
];

fn engines(key: &[u8]) -> Vec<Core> {
    let mut v = std::vec![Core::new(key, CpuFeatures::portable())];
    let hw = Core::new(key, CpuFeatures::compile_time());
    if hw.is_hardware() {
        v.push(hw);
    }
    v
}

#[test]
fn hardware_path_is_taken_when_compiled_in() {
    let hw = Core::new(&[0; 16], CpuFeatures::compile_time());
    let expected = cfg!(all(
        target_arch = "aarch64",
        target_feature = "aes",
        not(torajs_crypto_portable)
    )) || cfg!(all(
        target_arch = "x86_64",
        target_feature = "aes",
        target_feature = "pclmulqdq",
        target_feature = "ssse3",
        not(torajs_crypto_portable)
    ));
    assert_eq!(hw.is_hardware(), expected);
    assert!(!Core::new(&[0; 16], CpuFeatures::portable()).is_hardware());
}

#[test]
fn nist_examples() {
    for c in CASES {
        for e in engines(&unhex(c.key)) {
            let iv = unhex(c.iv);
            let aad = unhex(c.aad);
            let mut buf = unhex(c.pt);
            let tag = e.seal(&iv, &aad, &mut buf).unwrap();
            let hw = e.is_hardware();
            assert_eq!(hex(&buf), c.ct, "case {} hw {hw}", c.n);
            assert_eq!(hex(&tag), c.tag, "case {} hw {hw}", c.n);
            e.open(&iv, &aad, &mut buf, &tag).unwrap();
            assert_eq!(hex(&buf), c.pt, "case {} hw {hw}", c.n);
        }
    }
}

#[test]
fn open_rejects_and_wipes() {
    for e in engines(&[7; 32]) {
        let iv = [3u8; 12];
        let mut sealed = std::vec![0x5au8; 77];
        let tag = e.seal(&iv, b"aad", &mut sealed).unwrap();
        for i in (0..sealed.len() * 8).step_by(7) {
            let mut buf = sealed.clone();
            buf[i / 8] ^= 1 << (i % 8);
            assert_eq!(e.open(&iv, b"aad", &mut buf, &tag), Err(AeadError));
            assert!(buf.iter().all(|&b| b == 0), "bit {i}");
        }
        for i in 0..TAG_LEN * 8 {
            let mut bad = tag;
            bad[i / 8] ^= 1 << (i % 8);
            let mut buf = sealed.clone();
            assert!(e.open(&iv, b"aad", &mut buf, &bad).is_err());
        }
        let mut buf = sealed.clone();
        assert!(e.seal(&[], b"", &mut buf).is_err());
        assert_eq!(buf, sealed, "a refused seal leaves the buffer alone");
    }
}

/// xorshift64*; deterministic so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// The hardware and portable engines agree byte for byte on 10^5 random
/// inputs. On targets without the hardware path this compares the
/// portable engine with itself and still checks the round trip.
#[test]
fn hardware_and_portable_agree() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..100_000 {
        let key_len = [16, 24, 32][rng.below(3)];
        let key = rng.bytes(key_len);
        let iv_len = if rng.below(4) == 0 {
            1 + rng.below(70)
        } else {
            12
        };
        let iv = rng.bytes(iv_len);
        let aad_len = rng.below(90);
        let aad = rng.bytes(aad_len);
        let data_len = rng.below(300);
        let data = rng.bytes(data_len);
        let soft = Core::new(&key, CpuFeatures::portable());
        let hw = Core::new(&key, CpuFeatures::compile_time());
        let (mut a, mut b) = (data.clone(), data.clone());
        let ta = soft.seal(&iv, &aad, &mut a).unwrap();
        let tb = hw.seal(&iv, &aad, &mut b).unwrap();
        assert_eq!((&a, ta), (&b, tb), "round {round}");
        hw.open(&iv, &aad, &mut a, &ta).unwrap();
        soft.open(&iv, &aad, &mut b, &tb).unwrap();
        assert_eq!(a, data, "round {round}");
        assert_eq!(b, data, "round {round}");
    }
}

#[test]
fn debug_hides_the_key() {
    let k = Aes256Gcm::new(&[1; 32], CpuFeatures::portable());
    assert_eq!(std::format!("{k:?}"), "Aes256Gcm");
}
