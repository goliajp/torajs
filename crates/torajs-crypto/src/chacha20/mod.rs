//! The ChaCha20 stream cipher (RFC 8439 §2.3, §2.4), IETF variant: 256-bit
//! key, 96-bit nonce, 32-bit block counter.
//!
//! Two implementations of the bulk keystream: portable integer code
//! ([`soft`]) and AArch64 NEON ([`aarch64`]), which an [`Engine`] reaches
//! only when the caller's [`CpuFeatures`] vouches for
//! [`Feature::Aarch64Neon`](crate::cpu::Feature::Aarch64Neon). Both produce
//! identical output.

#[cfg(all(target_arch = "aarch64", not(torajs_crypto_portable)))]
mod aarch64;
mod soft;

pub(crate) use soft::{BLOCK_LEN, block, key_words, nonce_words};

use crate::cpu::CpuFeatures;

/// The keystream implementation a key was built with.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Engine {
    Soft,
    #[cfg(all(target_arch = "aarch64", not(torajs_crypto_portable)))]
    Neon(aarch64::Neon),
}

impl Engine {
    #[cfg_attr(
        not(all(target_arch = "aarch64", not(torajs_crypto_portable))),
        allow(unused_variables)
    )]
    pub(crate) fn new(cpu: CpuFeatures) -> Engine {
        #[cfg(all(target_arch = "aarch64", not(torajs_crypto_portable)))]
        if let Some(n) = aarch64::Neon::new(cpu) {
            return Engine::Neon(n);
        }
        Engine::Soft
    }

    /// XORs `data` with the keystream that starts at block `counter`.
    ///
    /// The caller bounds `data.len()` so the counter does not wrap.
    pub(crate) fn xor_keystream(
        self,
        key: &[u32; 8],
        counter: u32,
        nonce: &[u32; 3],
        data: &mut [u8],
    ) {
        match self {
            Engine::Soft => soft::xor_keystream(key, counter, nonce, data),
            #[cfg(all(target_arch = "aarch64", not(torajs_crypto_portable)))]
            Engine::Neon(n) => n.xor_keystream(key, counter, nonce, data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{hex, unhex};

    fn engines() -> [Engine; 2] {
        [
            Engine::new(CpuFeatures::portable()),
            Engine::new(CpuFeatures::compile_time()),
        ]
    }

    #[test]
    fn compile_time_picks_neon_where_the_target_has_it() {
        let soft = matches!(Engine::new(CpuFeatures::compile_time()), Engine::Soft);
        assert_eq!(
            !soft,
            cfg!(all(
                target_arch = "aarch64",
                target_feature = "neon",
                not(torajs_crypto_portable)
            ))
        );
    }

    /// RFC 8439 §2.4.2 on every engine.
    #[test]
    fn encryption() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let nonce = nonce_words(&unhex("000000000000004a00000000").try_into().unwrap());
        for e in engines() {
            let mut data = *b"Ladies and Gentlemen of the class of '99: If I could offer you \
only one tip for the future, sunscreen would be it.";
            e.xor_keystream(&key_words(&key), 1, &nonce, &mut data);
            assert_eq!(
                hex(&data),
                "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0b\
                 f91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d8\
                 07ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab7793736\
                 5af90bbf74a35be6b40b8eedf2785e42874d",
                "{e:?}"
            );
        }
    }

    /// Every length from 0 to 1100 bytes plus longer ones that reach each
    /// combination of three-group, two-group and one-group steps, with
    /// random keys, nonces, counters (some wrapping inside a group) and
    /// data.
    #[test]
    fn engines_agree_on_random_inputs() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let [soft, other] = engines();
        let long = [1280, 1407, 1535, 1600, 1792, 2047, 2304, 2560, 3071, 16401];
        for len in (0..=1100usize).chain(long) {
            let key: [u32; 8] = core::array::from_fn(|_| next() as u32);
            let nonce: [u32; 3] = core::array::from_fn(|_| next() as u32);
            let counter = match len % 3 {
                0 => next() as u32,
                1 => u32::MAX - (next() % 8) as u32,
                _ => 1,
            };
            let data: std::vec::Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let mut a = data.clone();
            let mut b = data;
            soft.xor_keystream(&key, counter, &nonce, &mut a);
            other.xor_keystream(&key, counter, &nonce, &mut b);
            assert_eq!(a, b, "len {len} counter {counter}");
        }
    }
}
