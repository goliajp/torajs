//! Criterion benches for `torajs-codec-hex` on the workspace's hot
//! shape — 32-byte input (SHA-256 digest) → 64-char hex string.
//!
//! Encode + decode are the two paths. Reporting numbers help future
//! polish work verify no regression after micro-optimization
//! attempts (e.g. unrolled loops, SIMD-by-4, bytemuck transmute,
//! etc. — none of which are in the lean 0.1.0 impl).

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_codec_hex::{decode, encode};

fn bench_encode_sha256(c: &mut Criterion) {
    let digest: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    c.bench_function("encode-32-byte-digest", |b| {
        b.iter(|| black_box(encode(black_box(digest))));
    });
}

fn bench_decode_64_char_hex(c: &mut Criterion) {
    let hex = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    c.bench_function("decode-64-char-hex", |b| {
        b.iter(|| black_box(decode(black_box(hex))));
    });
}

fn bench_encode_64_kib(c: &mut Criterion) {
    // Larger input — verifies the per-byte loop scales linearly and
    // doesn't accidentally re-allocate due to a missing
    // with_capacity hint.
    let bytes: Vec<u8> = (0..65_536).map(|i| (i & 0xff) as u8).collect();
    c.bench_function("encode-64-kib", |b| {
        b.iter(|| black_box(encode(black_box(&bytes))));
    });
}

criterion_group!(
    benches,
    bench_encode_sha256,
    bench_decode_64_char_hex,
    bench_encode_64_kib
);
criterion_main!(benches);
