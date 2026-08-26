//! The byte-level half of `dead_strip_elide`: locate a conditional
//! site's relocs in its fn and rewrite the instruction words once the
//! judgment says the site is assumed away. Mechanism only; the two
//! shapes are `SiteShape`'s.

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};

use crate::dead_strip_elide::{ElidableSite, SiteShape};

const NOP: u32 = 0xD503_201F;
/// `movz Xd, #0` with `Rd` in the low five bits.
const MOVZ_X_ZERO: u32 = 0xD280_0000;

/// Rewrite `site` in `funcs`. A missing fn, reloc, or an instruction
/// word of the wrong shape is a caller contract violation — loud,
/// never skipped.
pub(crate) fn patch_site(
    funcs: &mut [CompiledFunction],
    site: &ElidableSite,
) -> Result<(), String> {
    let f = funcs
        .iter_mut()
        .find(|f| f.name == site.func)
        .ok_or_else(|| format!("elidable site: no fn named {}", site.func))?;
    match &site.shape {
        SiteShape::Call {
            byte_offset,
            replacement,
        } => patch_call(f, *byte_offset, *replacement),
        SiteShape::FnAddr {
            adrp_offset,
            target,
        } => patch_fn_addr(f, *adrp_offset, target),
    }
}

fn word_at(f: &CompiledFunction, off: u32) -> Result<u32, String> {
    let off = off as usize;
    f.bytes
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("elidable site: {}+{off:#x} past fn end", f.name))
}

fn reloc_index(
    f: &CompiledFunction,
    off: u32,
    want: impl Fn(&RelocKind) -> bool,
    what: &str,
) -> Result<usize, String> {
    f.relocs
        .iter()
        .position(|r| r.byte_offset == off && want(&r.kind))
        .ok_or_else(|| format!("elidable site: {}+{off:#x} carries no {what} reloc", f.name))
}

/// Overwrite the `bl` word with the replacement and drop its reloc.
fn patch_call(f: &mut CompiledFunction, off: u32, replacement: u32) -> Result<(), String> {
    let idx = reloc_index(
        f,
        off,
        |k| {
            matches!(
                k,
                RelocKind::CallSite {
                    target: CallTarget::Extern(_)
                }
            )
        },
        "extern CallSite",
    )?;
    let word = word_at(f, off)?;
    // BL: 100101 imm26.
    if word & 0xFC00_0000 != 0x9400_0000 {
        return Err(format!(
            "elidable site: {}+{off:#x} is {word:#010x}, not a bl",
            f.name
        ));
    }
    let o = off as usize;
    f.bytes[o..o + 4].copy_from_slice(&replacement.to_le_bytes());
    f.relocs.remove(idx);
    Ok(())
}

/// `adrp Xd, page(sym)` → `movz Xd, #0`; `add Xd, Xd, #lo12(sym)` →
/// `nop`; both relocs dropped.
fn patch_fn_addr(f: &mut CompiledFunction, off: u32, target: &str) -> Result<(), String> {
    let page = reloc_index(
        f,
        off,
        |k| matches!(k, RelocKind::Page21 { target_sym } if target_sym == target),
        "Page21",
    )?;
    let pageoff = reloc_index(
        f,
        off + 4,
        |k| matches!(k, RelocKind::PageOff12 { target_sym } if target_sym == target),
        "PageOff12",
    )?;
    let adrp = word_at(f, off)?;
    let add = word_at(f, off + 4)?;
    // ADRP: 1 immlo(2) 10000 immhi(19) Rd(5); ADD (imm, 64-bit, sh=0):
    // 1001000100 imm12 Rn Rd.
    let rd = adrp & 0x1F;
    if adrp & 0x9F00_0000 != 0x9000_0000 {
        return Err(format!(
            "elidable site: {}+{off:#x} is {adrp:#010x}, not an adrp",
            f.name
        ));
    }
    if add & 0xFFC0_0000 != 0x9100_0000 || add & 0x1F != rd || (add >> 5) & 0x1F != rd {
        return Err(format!(
            "elidable site: {}+{:#x} is {add:#010x}, not `add x{rd}, x{rd}, #imm`",
            f.name,
            off + 4
        ));
    }
    let o = off as usize;
    f.bytes[o..o + 4].copy_from_slice(&(MOVZ_X_ZERO | rd).to_le_bytes());
    f.bytes[o + 4..o + 8].copy_from_slice(&NOP.to_le_bytes());
    // Higher index first so the lower one stays valid.
    for idx in [page.max(pageoff), page.min(pageoff)] {
        f.relocs.remove(idx);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dead_strip_elide::Guard;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::Reloc;

    fn fn_with_bl(name: &str, callee: &str) -> CompiledFunction {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&NOP.to_le_bytes());
        bytes.extend_from_slice(&0x9400_0000u32.to_le_bytes());
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs: vec![Reloc {
                byte_offset: 4,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(callee.into()),
                },
            }],
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    fn call_site(func: &str, off: u32) -> ElidableSite {
        ElidableSite {
            func: func.into(),
            guard: Guard::Symbols(Vec::new()),
            shape: SiteShape::Call {
                byte_offset: off,
                replacement: NOP,
            },
        }
    }

    #[test]
    fn patch_rewrites_bl_and_drops_reloc() {
        let mut funcs = vec![fn_with_bl("_main_user", "___torajs_drain")];
        patch_site(&mut funcs, &call_site("_main_user", 4)).unwrap();
        assert_eq!(&funcs[0].bytes[4..8], &NOP.to_le_bytes());
        assert!(funcs[0].relocs.is_empty());
    }

    #[test]
    fn patch_refuses_non_bl_word_and_unknown_sites() {
        let mut funcs = vec![fn_with_bl("_main_user", "___torajs_drain")];
        funcs[0].relocs[0].byte_offset = 0;
        let err = patch_site(&mut funcs, &call_site("_main_user", 0)).unwrap_err();
        assert!(err.contains("not a bl"), "{err}");
        assert!(patch_site(&mut funcs, &call_site("_main_user", 8)).is_err());
        assert!(patch_site(&mut funcs, &call_site("_other", 0)).is_err());
    }

    #[test]
    fn patch_fn_addr_checks_the_pair_shape() {
        // add's Rd is x10, not the adrp's x9.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(0x9000_0000u32 | 9).to_le_bytes());
        bytes.extend_from_slice(&(0x9100_0000u32 | (9 << 5) | 10).to_le_bytes());
        let t = "__torajs_boxed_0".to_string();
        let mut funcs = vec![CompiledFunction {
            name: "m".into(),
            bytes,
            relocs: vec![
                Reloc {
                    byte_offset: 0,
                    kind: RelocKind::Page21 {
                        target_sym: t.clone(),
                    },
                },
                Reloc {
                    byte_offset: 4,
                    kind: RelocKind::PageOff12 {
                        target_sym: t.clone(),
                    },
                },
            ],
            frame: FrameLayout::leaf_no_spill(),
        }];
        let site = ElidableSite {
            func: "m".into(),
            guard: Guard::Symbols(Vec::new()),
            shape: SiteShape::FnAddr {
                adrp_offset: 0,
                target: t,
            },
        };
        let err = patch_site(&mut funcs, &site).unwrap_err();
        assert!(err.contains("not `add x9, x9"), "{err}");
        assert_eq!(funcs[0].relocs.len(), 2, "refused sites stay untouched");
    }
}
