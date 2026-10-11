//! Sealing throughput: 65536 records of 16 KiB each (1 GiB), the shape of
//! a bulk TLS transfer. A timing measurement, so ignored by default:
//!
//! ```text
//! cargo test --release --test throughput -- --ignored --nocapture
//! ```

use std::hint::black_box;
use std::time::Instant;

use torajs_crypto::Aead;
use torajs_crypto::chacha20poly1305::ChaCha20Poly1305;
use torajs_crypto::cpu::CpuFeatures;
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
