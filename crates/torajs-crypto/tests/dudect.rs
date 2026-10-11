//! Statistical constant-time checks in the style of dudect (Reparaz,
//! Balasch and Verbauwhede, "Dude, is my code constant time?", 2017).
//!
//! Each check times an operation on two classes of secret input, a fixed
//! one and a random one, interleaved in random order, and applies Welch's
//! t-test to the two timing distributions. `|t| > 10` means the running
//! time depends on the secret. The checks are timing measurements, so
//! they are ignored by default; run them on an idle machine with
//!
//! ```text
//! cargo test --release --test dudect -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `DUDECT_SAMPLES` sets the number of measurements per check (default
//! one million).

use std::hint::black_box;
use std::time::Instant;

use torajs_crypto::Aead;
use torajs_crypto::chacha20poly1305::ChaCha20Poly1305;
use torajs_crypto::cpu::CpuFeatures;
use torajs_crypto::gcm::Aes128Gcm;

/// Calls per measurement, so one sample spans many timer ticks.
const BATCH: usize = 8;
const LEN: usize = 64;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn fill(&mut self, b: &mut [u8]) {
        for x in b {
            *x = self.next() as u8;
        }
    }
}

#[derive(Default)]
struct Moments {
    n: f64,
    mean: f64,
    m2: f64,
}

impl Moments {
    fn push(&mut self, x: f64) {
        self.n += 1.0;
        let d = x - self.mean;
        self.mean += d / self.n;
        self.m2 += d * (x - self.mean);
    }

    fn var(&self) -> f64 {
        self.m2 / (self.n - 1.0)
    }
}

fn welch(a: &Moments, b: &Moments) -> f64 {
    (a.mean - b.mean) / (a.var() / a.n + b.var() / b.n).sqrt()
}

/// Times `op` on `samples` inputs from `input(class, rng, buf)` and returns
/// the largest |t| over the uncropped samples and samples cropped at the
/// 50th, 75th and 90th percentile (dudect crops to drop interrupts and
/// other long-tail noise).
fn run(
    name: &str,
    mut input: impl FnMut(usize, &mut Rng, &mut [u8; LEN]),
    mut op: impl FnMut(&mut [u8; LEN]),
) -> f64 {
    let samples: usize = std::env::var("DUDECT_SAMPLES")
        .ok()
        .map(|s| s.parse().expect("DUDECT_SAMPLES"))
        .unwrap_or(1_000_000);
    let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ name.len() as u64);
    let mut classes = Vec::with_capacity(samples);
    let mut inputs = Vec::with_capacity(samples);
    for _ in 0..samples {
        let class = (rng.next() & 1) as usize;
        let mut buf = [0u8; LEN];
        input(class, &mut rng, &mut buf);
        classes.push(class);
        inputs.push(buf);
    }
    let mut times = Vec::with_capacity(samples);
    for x in &inputs {
        // each call gets its own copy, made before the timer starts: the
        // copy itself (fetching the input, storing it) runs faster on some
        // byte patterns and is not part of the operation under test
        let mut bufs = black_box([*x; BATCH]);
        let start = Instant::now();
        for b in &mut bufs {
            op(black_box(b));
        }
        times.push(start.elapsed().as_nanos() as f64);
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let mut worst = 0.0f64;
    for pct in [100, 90, 75, 50] {
        let cut = sorted[(samples - 1) * pct / 100];
        let mut m = [Moments::default(), Moments::default()];
        for (&t, &c) in times.iter().zip(&classes) {
            if t <= cut {
                m[c].push(t);
            }
        }
        let t = welch(&m[0], &m[1]);
        println!(
            "{name}: crop p{pct} n={}/{} mean {:.1}/{:.1} ns t={t:.2}",
            m[0].n, m[1].n, m[0].mean, m[1].mean
        );
        worst = worst.max(t.abs());
    }
    worst
}

/// The tag-check failure path of `open`: class 0 is a tag that matches in
/// every byte but the last, class 1 a random tag. An early-exit comparison
/// would take longer on class 0.
fn open_fail<K: Aead>(name: &str, key: &K) -> f64 {
    let nonce = [5u8; 12];
    let mut sealed = [0x33u8; LEN];
    let good = key.seal(&nonce, b"", &mut sealed).unwrap();
    // the input buffer carries the tag in its first 16 bytes
    run(
        name,
        |class, rng, buf| {
            if class == 0 {
                buf[..16].copy_from_slice(&good);
                buf[15] ^= 1;
            } else {
                rng.fill(&mut buf[..16]);
            }
        },
        |x| {
            let tag: [u8; 16] = x[..16].try_into().unwrap();
            let mut ct = sealed;
            assert!(key.open(&nonce, b"", &mut ct, &tag).is_err());
        },
    )
}

/// Sealing secret data: class 0 a fixed buffer (all zeros, or fixed
/// random-looking bytes), class 1 random bytes.
fn seal_data<K: Aead>(name: &str, key: &K, zero: bool) -> f64 {
    let nonce = [6u8; 12];
    let mut fixed = [0u8; LEN];
    if !zero {
        Rng(42).fill(&mut fixed);
    }
    run(
        name,
        |class, rng, buf| {
            if class == 0 {
                *buf = fixed;
            } else {
                rng.fill(buf);
            }
        },
        |buf| {
            black_box(key.seal(&nonce, b"", buf).unwrap());
        },
    )
}

/// Key setup and sealing under a secret key: class 0 a fixed key, class 1
/// a random key. The first `key_len` bytes of the input are the key.
fn seal_key<K: Aead>(name: &str, key_len: usize, new: impl Fn(&[u8]) -> K) -> f64 {
    let nonce = [8u8; 12];
    run(
        name,
        |class, rng, buf| {
            if class == 0 {
                buf[..key_len].fill(0x5c);
            } else {
                rng.fill(&mut buf[..key_len]);
            }
        },
        |x| {
            let key = new(&x[..key_len]);
            let mut msg = [0x33u8; LEN];
            black_box(key.seal(&nonce, b"", &mut msg).unwrap());
        },
    )
}

fn assert_ct(t: f64) {
    println!("max |t| {t:.2}");
    assert!(t < 10.0, "|t| = {t:.2}");
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn gcm_open_tag_check_portable() {
    let key = Aes128Gcm::new(&[0x11; 16], CpuFeatures::portable());
    assert_ct(open_fail("gcm-open-portable", &key));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn gcm_open_tag_check_compile_time() {
    let key = Aes128Gcm::new(&[0x11; 16], CpuFeatures::compile_time());
    assert_ct(open_fail("gcm-open-hw", &key));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn chacha_open_tag_check_portable() {
    let key = ChaCha20Poly1305::new(&[0x22; 32], CpuFeatures::portable());
    assert_ct(open_fail("chacha-open-portable", &key));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn chacha_open_tag_check_compile_time() {
    let key = ChaCha20Poly1305::new(&[0x22; 32], CpuFeatures::compile_time());
    assert_ct(open_fail("chacha-open-hw", &key));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn gcm_seal_data_portable() {
    let key = Aes128Gcm::new(&[0x11; 16], CpuFeatures::portable());
    assert_ct(seal_data("gcm-seal-portable", &key, false));
    assert_ct(seal_data("gcm-seal-portable-zero", &key, true));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn chacha_seal_data_portable() {
    let key = ChaCha20Poly1305::new(&[0x22; 32], CpuFeatures::portable());
    assert_ct(seal_data("chacha-seal-portable", &key, false));
    assert_ct(seal_data("chacha-seal-portable-zero", &key, true));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn chacha_seal_data_compile_time() {
    let key = ChaCha20Poly1305::new(&[0x22; 32], CpuFeatures::compile_time());
    assert_ct(seal_data("chacha-seal-hw", &key, false));
    assert_ct(seal_data("chacha-seal-hw-zero", &key, true));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn gcm_key_portable() {
    assert_ct(seal_key("gcm-key-portable", 16, |k| {
        Aes128Gcm::new(k.try_into().unwrap(), CpuFeatures::portable())
    }));
}

#[test]
#[ignore = "timing measurement; run on an idle machine"]
fn chacha_key() {
    assert_ct(seal_key("chacha-key", 32, |k| {
        ChaCha20Poly1305::new(k.try_into().unwrap(), CpuFeatures::compile_time())
    }));
}
