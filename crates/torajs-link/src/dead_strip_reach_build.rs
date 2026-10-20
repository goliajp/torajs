//! S2 dead-strip reach — member parsing half (extracted from
//! `dead_strip_reach` when the why-live tracing pushed the closure
//! file at the 500-line hard cap). Answers "what are this member's
//! atoms and symbols"; the closure file answers "which of them are
//! reachable".

use torajs_obj::MachHeader64;

use crate::archive::ArMember;
use crate::dead_strip_reach::MemberReach;
use crate::dead_strip_reach_encode::encode_member_reach;
use crate::dead_strip_reach_table::reach_from_table;

/// Parse one member into its reachability tables.
pub(crate) fn build_member_reach<'a>(member: &ArMember<'a>) -> Result<MemberReach<'a>, String> {
    Ok(reach_from_table(
        member,
        &encode_member_reach(member, &|_| None)?,
    ))
}

/// Whether the object `bytes` declares `MH_SUBSECTIONS_VIA_SYMBOLS`,
/// so its sections may be cut at any symbol.
pub(crate) fn splits_at_symbols(bytes: &[u8]) -> bool {
    MachHeader64::parse(bytes).is_some_and(|h| h.splits_at_symbols())
}

/// Trim trailing NUL/space padding off a raw 16-byte name slot.
pub(crate) fn trim16(raw: &[u8; 16]) -> &[u8] {
    let end = raw
        .iter()
        .position(|&b| b == 0 || b == b' ')
        .unwrap_or(raw.len());
    &raw[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;

    fn leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// `__text` atoms, section-relative.
    fn text_atoms(obj: &[u8]) -> Vec<(u64, u64)> {
        let member = ArMember {
            name: "t.o",
            data: obj,
        };
        let reach = build_member_reach(&member).unwrap();
        let text = reach.sects.iter().find(|s| s.is_text).unwrap();
        text.atoms
            .iter()
            .map(|&(s, e)| (s - text.addr, e - text.addr))
            .collect()
    }

    #[test]
    fn only_a_subsections_object_splits_at_its_symbols() {
        let mut obj = torajs_obj::write_object(&[leaf("_a"), leaf("_b"), leaf("_c")]);
        assert!(!splits_at_symbols(&obj));
        assert_eq!(text_atoms(&obj), [(0, 12)]);

        let mut header = MachHeader64::parse(&obj).unwrap();
        header.flags |= torajs_obj::MH_SUBSECTIONS_VIA_SYMBOLS;
        header.write_over(&mut obj);
        assert!(splits_at_symbols(&obj));
        assert_eq!(text_atoms(&obj), [(0, 4), (4, 8), (8, 12)]);
    }
}
