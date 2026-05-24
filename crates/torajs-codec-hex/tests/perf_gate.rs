//! Performance regression gates for `torajs-codec-hex`. See [BUDGETS.md].
//!
//! Hex encode/decode is a leaf utility; the workspace's hot use is
//! 32-byte SHA-256-digest formatting. Budgets target 5× headroom over
//! observed ~60-150 ns per call.
//!
//! Run with `cargo test -p torajs-codec-hex --test perf_gate --release`.

use std::time::{Duration, Instant};

use torajs_codec_hex::{decode, encode};

fn time_median<F: FnMut()>(mut op: F, samples: usize) -> Duration {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        op();
        times.push(start.elapsed());
    }
    times.sort();
    times[samples / 2]
}

const SHA256_DIGEST: [u8; 32] = [
    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
];

#[test]
fn encode_32byte_digest_100k_under_budget() {
    // BUDGETS.md target: ~60 ns / call → 100k calls ≤ 10 ms.
    // CI budget: 50 ms (5× headroom).
    let digest = SHA256_DIGEST;
    let median = time_median(
        || {
            for _ in 0..100_000 {
                let s = encode(digest);
                std::hint::black_box(s);
            }
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "encode 32-byte regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn decode_64char_100k_under_budget() {
    // BUDGETS.md target: ~150 ns / call → 100k calls ≤ 20 ms.
    // CI budget: 100 ms.
    let hex = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let median = time_median(
        || {
            for _ in 0..100_000 {
                let v = decode(hex);
                let _ = std::hint::black_box(v);
            }
        },
        11,
    );
    let budget = Duration::from_millis(100);
    assert!(
        median < budget,
        "decode 64-char regressed: median {median:?} >= budget {budget:?}"
    );
}
