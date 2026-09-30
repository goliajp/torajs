//! §22.2.2.7.3 Canonicalize over one UTF-16 code unit, for the units
//! a Latin-1 atom can meet.
//!
//! Without `u` / `v` a character canonicalizes to its `toUpperCase`
//! when that is one code unit, except that a unit at or above 0x80
//! never becomes an ASCII one (so `ſ` stays `ſ` and `ı` stays `ı`).
//! With `u` / `v` it is the simple case folding of CaseFolding.txt
//! (statuses C and S).
//!
//! The lane's atoms are Latin-1, so the only units above 0xFF that
//! matter are the ones sharing a canonical form with a Latin-1 unit:
//! `Ÿ`, `ſ`, `ẞ`, `K` (Kelvin), `Å` (Angstrom), `Μ` and `μ`. Every
//! other unit above 0xFF is answered as its own class, which decides
//! membership against a Latin-1 atom exactly.

/// Units above 0xFF whose canonical form is shared with a Latin-1
/// unit under one of the two modes.
pub const SHARED_HIGH_UNITS: [u32; 7] = [0x178, 0x17F, 0x1E9E, 0x212A, 0x212B, 0x39C, 0x3BC];

/// The canonical form of `u` (`unicode` picks the folding).
pub fn canonicalize(u: u32, unicode: bool) -> u32 {
    if unicode {
        return match u {
            0x41..=0x5A => u + 0x20,
            0xB5 => 0x3BC,
            0xC0..=0xD6 | 0xD8..=0xDE => u + 0x20,
            0x178 => 0xFF,
            0x17F => 0x73,
            0x1E9E => 0xDF,
            0x212A => 0x6B,
            0x212B => 0xE5,
            0x39C => 0x3BC,
            _ => u,
        };
    }
    match u {
        0x61..=0x7A => u - 0x20,
        0xB5 => 0x39C,
        0xE0..=0xF6 | 0xF8..=0xFE => u - 0x20,
        0xFF => 0x178,
        0x3BC => 0x39C,
        _ => u,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: u32, b: u32, unicode: bool) -> bool {
        canonicalize(a, unicode) == canonicalize(b, unicode)
    }

    #[test]
    fn ascii_pairs_fold_in_both_modes() {
        for u in 0x61..=0x7Au32 {
            assert!(same(u, u - 0x20, false));
            assert!(same(u, u - 0x20, true));
        }
        assert!(!same(b'a' as u32, b'b' as u32, false));
        assert!(!same(b'_' as u32, b'?' as u32, true));
    }

    #[test]
    fn latin1_letters_fold_and_the_odd_ones_follow_the_spec() {
        assert!(same(0xE9, 0xC9, false));
        assert!(same(0xE9, 0xC9, true));
        assert!(!same(0xF7, 0xD7, false));
        assert!(!same(0xF7, 0xD7, true));
        // ß uppercases to two units, so it is its own class; ẞ folds to it
        assert!(!same(0xDF, 0x1E9E, false));
        assert!(same(0xDF, 0x1E9E, true));
        // ÿ and Ÿ
        assert!(same(0xFF, 0x178, false));
        assert!(same(0xFF, 0x178, true));
        // µ, Μ and μ
        assert!(same(0xB5, 0x39C, false));
        assert!(same(0xB5, 0x3BC, false));
        assert!(same(0xB5, 0x3BC, true));
        assert!(same(0xB5, 0x39C, true));
    }

    #[test]
    fn ascii_never_gains_a_high_twin_without_u() {
        assert!(!same(b's' as u32, 0x17F, false));
        assert!(same(b's' as u32, 0x17F, true));
        assert!(!same(b'k' as u32, 0x212A, false));
        assert!(same(b'k' as u32, 0x212A, true));
        assert!(!same(0xE5, 0x212B, false));
        assert!(same(0xE5, 0x212B, true));
        assert!(!same(b'i' as u32, 0x131, false));
        assert!(!same(b'i' as u32, 0x130, true));
    }

    #[test]
    fn every_other_high_unit_is_its_own_class() {
        for u in 0x100..=0xFFFFu32 {
            if SHARED_HIGH_UNITS.contains(&u) {
                continue;
            }
            assert_eq!(canonicalize(u, false), u);
            assert_eq!(canonicalize(u, true), u);
        }
        for &u in &SHARED_HIGH_UNITS {
            let shared = |unicode: bool| {
                let c = canonicalize(u, unicode);
                (0..=0xFFu32).any(|l| canonicalize(l, unicode) == c)
            };
            assert!(shared(false) || shared(true), "{u:#x}");
        }
    }
}
