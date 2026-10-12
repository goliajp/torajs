//! The AES block cipher (FIPS 197), encryption direction only, for AES-128,
//! AES-192 and AES-256. GCM is its only user, so no block-cipher mode is
//! exported.
//!
//! Three implementations: a bitsliced portable one ([`soft`]), one on the
//! ARMv8 AES instructions (`aarch64`) and one on AES-NI (`x86_64`); a
//! caller reaches the last two only through a
//! [`CpuFeatures`](crate::cpu::CpuFeatures) value that vouches for them.
#[cfg(all(target_arch = "aarch64", not(torajs_crypto_portable)))]
pub(crate) mod aarch64;
pub(crate) mod soft;
#[cfg(all(
    target_arch = "x86_64",
    target_feature = "sse2",
    not(torajs_crypto_portable)
))]
pub(crate) mod x86_64;

pub(crate) use soft::expand_key;

pub(crate) const BLOCK_LEN: usize = 16;
pub(crate) const MAX_ROUNDS: usize = 14;
/// Expanded key words for the largest key: `4 * (MAX_ROUNDS + 1)`.
pub(crate) const MAX_WORDS: usize = 4 * (MAX_ROUNDS + 1);

#[cfg(test)]
mod tests;
