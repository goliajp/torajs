//! Sealing throughput: 65536 records of 16 KiB each (1 GiB), the shape of
//! a bulk TLS transfer, and the fixed cost per call of switching on
//! data-independent timing, measured on 64-byte records. Timing
//! measurements, so ignored by default:
//!
//! ```text
//! cargo test --release --test throughput -- --ignored --nocapture
//! ```

use std::hint::black_box;
use std::time::Instant;

use torajs_crypto::Aead;
use torajs_crypto::chacha20poly1305::ChaCha20Poly1305;
use torajs_crypto::cpu::{CpuFeatures, Feature, with_data_independent_timing};
use torajs_crypto::gcm::{Aes128Gcm, Aes256Gcm};

const RECORD: usize = 16 * 1024;
const RECORDS: u32 = 64 * 1024;

fn measure<K: Aead>(name: &str, key: &K, records: u32) {
    let mut buf = vec![0x5au8; RECORD];
    let aad = [0x17, 0x03, 0x03, 0x40, 0x11];
    let start = Instant::now();
    let mut nonce = [0u8; 12];
    for i in 0..records {
        nonce[8..].copy_from_slice(&i.to_be_bytes());
        black_box(key.seal(&nonce, &aad, &mut buf).unwrap());
    }
    let secs = start.elapsed().as_secs_f64();
    let bytes = RECORD as f64 * f64::from(records);
    println!(
        "{name:28} {records:6} x 16 KiB  {:8.1} ms  {:6.2} GB/s",
        secs * 1e3,
        bytes / secs / 1e9
    );
}

#[test]
#[ignore = "timing measurement"]
fn seal_16k_records() {
    let hw = CpuFeatures::compile_time();
    let soft = CpuFeatures::portable();
    measure(
        "aes128gcm compile-time",
        &Aes128Gcm::new(&[1; 16], hw),
        RECORDS,
    );
    measure(
        "aes256gcm compile-time",
        &Aes256Gcm::new(&[1; 32], hw),
        RECORDS,
    );
    measure(
        "chacha20poly1305 compile-time",
        &ChaCha20Poly1305::new(&[1; 32], hw),
        RECORDS / 8,
    );
    measure(
        "chacha20poly1305 portable",
        &ChaCha20Poly1305::new(&[1; 32], soft),
        RECORDS / 8,
    );
    measure(
        "aes128gcm portable",
        &Aes128Gcm::new(&[1; 16], soft),
        RECORDS / 32,
    );
}

fn small<K: Aead>(name: &str, key: &K, outer: CpuFeatures) {
    const N: u32 = 1 << 21;
    let mut buf = [0x5au8; 64];
    let mut nonce = [0u8; 12];
    let start = Instant::now();
    with_data_independent_timing(outer, || {
        for i in 0..N {
            nonce[8..].copy_from_slice(&i.to_be_bytes());
            black_box(key.seal(&nonce, b"", &mut buf).unwrap());
        }
    });
    let ns = start.elapsed().as_secs_f64() * 1e9 / f64::from(N);
    println!("{name:44} {ns:7.1} ns per 64-byte seal");
}

#[test]
#[ignore = "timing measurement"]
fn seal_64b_records_dit_cost() {
    let hw = CpuFeatures::compile_time();
    let no_dit = hw.without(Feature::Aarch64Dit);
    let none = CpuFeatures::portable();
    small(
        "aes128gcm without dit",
        &Aes128Gcm::new(&[1; 16], no_dit),
        none,
    );
    small(
        "aes128gcm dit per call",
        &Aes128Gcm::new(&[1; 16], hw),
        none,
    );
    small(
        "aes128gcm dit per call, outer scope",
        &Aes128Gcm::new(&[1; 16], hw),
        hw,
    );
    small(
        "chacha20poly1305 without dit",
        &ChaCha20Poly1305::new(&[1; 32], no_dit),
        none,
    );
    small(
        "chacha20poly1305 dit per call",
        &ChaCha20Poly1305::new(&[1; 32], hw),
        none,
    );
    small(
        "chacha20poly1305 dit per call, outer scope",
        &ChaCha20Poly1305::new(&[1; 32], hw),
        hw,
    );
}
