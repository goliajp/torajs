//! The `__text` payload of an archive-linked executable: each
//! integrated member's patched `__text` slice, then the user functions
//! (relocations resolved).
//!
//! The runtime members go first so that where they land does not hang
//! on the program's own code: with the user functions in front, every
//! byte a user function gained or lost moved the whole runtime with it,
//! and a timing that depends on the distance between two functions
//! (aliasing in a table indexed by low address bits, period 2 KiB on
//! Apple Silicon) changed with an edit to code that never runs in the
//! loop. In front, the runtime's placement depends only on the members
//! the program links; a user function still moves with the user code
//! before it.
//!
//! Every user function and every member starts on a
//! [`TEXT_FN_ALIGN`] boundary. Packed back to back, a function's
//! offset within a fetch line depended on the byte length of every
//! function laid out before it, so a change to one function moved the
//! code of its neighbours and their timings with it. The gaps are
//! zero-filled (`udf #0`): nothing branches into them.
//!
//! A user function with loops may start later still, by whole fetch
//! lines, so that no loop of its straddles a 4 KiB boundary of the
//! address space ([`LOOP_REGION`]). The size is a measured rule, not a
//! documented one: objlit-optional-field-1m, one release tr, its
//! 392-byte hot loop moved by the link gap through 0x7ef0..0xcef0 in
//! 321 steps of 64 bytes (N=5, fastest run per step) read 1.124-1.143
//! of the fastest step at each of the 16 steps that put the loop's
//! first 336 bytes across a 4 KiB boundary that is not a 16 KiB page
//! boundary (0x9000, 0xa000, 0xb000, 0xd000), 1.125-1.142 at the 10
//! across a page boundary (0x8000, 0xc000), and at most 1.071 at the
//! 295 others, 2 KiB crossings and the 5 steps where only the loop's
//! last 56 bytes crossed among them. The effect is not general: the
//! same scan of collatz, whose hot loop is 132 bytes of four
//! interleaved branchless chains (main_user+0x4dc), read 1.005-1.043
//! at the 10 steps that put it across a 4 KiB boundary and 1.000-1.057
//! at the 301 that put it across none; for a loop like that the rule
//! costs only the pad, which sits in the `__TEXT` page slack. Where
//! the function lands within the region depended on the runtime
//! members and the user functions in front of it, so an edit anywhere
//! before a loop moved its reading. The loops come from codegen and
//! are read only for a function whose bytes would cross a region
//! boundary, the one case a loop of it can straddle. Where no start keeps every loop whole, the
//! one keeping the shortest whole is taken, then the next shortest,
//! and so on; the pad is at most the longest straddling loop rounded
//! up to a fetch line ([`loop_pad`]).
//!
//! Aligning a loop's head does not stand in for this: every step of
//! the scan moved the loop by a whole fetch line, so its head kept one
//! offset within a 64-byte line throughout, and the readings still
//! split by whether the loop crossed a 4 KiB boundary. The rule is of
//! the kind of the padding compilers emit for Intel's JCC erratum
//! (`-mbranches-within-32B-boundaries`: no jump may cross or end on a
//! 32-byte boundary), at the granularity this boundary was measured
//! at. Aligning every function with a loop to a power of two at least
//! the loop's length would also keep the loop whole, but it pads every
//! such function, on average by half that alignment, where the rule
//! pads only one that would straddle.
//!
//! `TORAJS_LINK_LOOP_PLACE_STATS=1` prints one line per archive
//! layout (`archive_layout_text`), the functions moved and the bytes
//! left in front of them; a `tr build` lays its text out once, after
//! dead strip, so its one line is the artifact's.
//! `TORAJS_LINK_LOOP_PLACE_OFF=1` packs as before (bisect gate).
//!
//! A loud-reject stub (a judged-away entry that only tail-branches
//! into the reject landing pad) never runs in a correctly judged
//! program and has no timing to keep, so it packs on instruction
//! alignment; the next function still opens a fetch line. So does an
//! answer-0 stub ([`ANSWER_ZERO_STUB`]): a judged-away entry whose
//! callers take 0 as "nothing here", two instructions with no
//! neighbour to move.

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};

use crate::archive_emit::pad_to;
use crate::archive_emit_members::MemberText;
use crate::archive_link::{ArchiveLayout, ArchiveLayoutError};
use crate::resolve::ResolvedFunction;

/// One Apple Silicon fetch line.
pub const TEXT_FN_ALIGN: u32 = 64;

/// Per user function name, the byte ranges `[start, end)` of its
/// loops, shortest first ([`crate::LinkConfig::user_loops`]).
pub type UserLoops = [(String, Vec<(u32, u32)>)];

/// The region a loop is kept inside of.
pub const LOOP_REGION: u32 = 4096;

/// Mach-O names of the reject landing pads share this prefix
/// (`torajs-dispatch`: `__torajs_dispatch_stub_reject` and its
/// pointer-answering twin).
const REJECT_PAD_PREFIX: &str = "___torajs_dispatch_stub_reject";

/// The body of an answer-0 stub: `movz x0, #0` + `ret`.
pub const ANSWER_ZERO_STUB: [u8; 8] = [0x00, 0x00, 0x80, 0xD2, 0xC0, 0x03, 0x5F, 0xD6];

/// What placing a user function needs to know of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextSlot<'a> {
    /// Its byte length.
    pub(crate) size: u32,
    /// A reject stub (`movz x7, #fam` + `b <pad>`) or an answer-0
    /// stub, which pack on instruction alignment.
    pub(crate) packed: bool,
    /// Its name, its loops are found by.
    pub(crate) name: &'a str,
}

/// `f`'s placement.
pub(crate) fn text_slot(f: &CompiledFunction) -> TextSlot<'_> {
    let zero = f.bytes == ANSWER_ZERO_STUB && f.relocs.is_empty();
    let packed = zero
        || f.bytes.len() == 8
            && matches!(
                f.relocs.as_slice(),
                [r] if r.byte_offset == 4
                    && matches!(&r.kind, RelocKind::CallSite { target: CallTarget::Extern(pad) }
                        if pad.starts_with(REJECT_PAD_PREFIX))
            );
    TextSlot {
        size: f.bytes.len() as u32,
        packed,
        name: &f.name,
    }
}

/// Offsets relative to the start of `__text`.
pub(crate) struct TextPlacement {
    pub(crate) user: Vec<u32>,
    pub(crate) members: Vec<u32>,
    /// End of the last placed byte (no trailing pad).
    pub(crate) size: u32,
    /// The user functions [`loop_pad`] moved, and the bytes it left in
    /// front of them.
    pub(crate) loop_pads: (u32, u32),
}

impl TextPlacement {
    /// `self`, its [`Self::loop_pads`] printed under
    /// `TORAJS_LINK_LOOP_PLACE_STATS`.
    pub(crate) fn reported(self) -> Self {
        if std::env::var_os("TORAJS_LINK_LOOP_PLACE_STATS").is_some() {
            let (n, bytes) = self.loop_pads;
            eprintln!("torajs-link-loop-place-stats: funcs_padded={n} pad_bytes={bytes}");
        }
        self
    }
}

/// `user_slots` are [`text_slot`]s, in function order. `user_gap` (a
/// multiple of [`TEXT_FN_ALIGN`]) empty bytes separate the member run
/// from the user functions. `text_base` is the address `__text` starts
/// at, modulo anything a multiple of [`LOOP_REGION`]: the file offset
/// serves, the segment being mapped at a region-aligned address.
/// `loops` are the user functions' loops by name
/// ([`crate::LinkConfig::user_loops`]).
pub(crate) fn place_text<'a>(
    user_slots: impl Iterator<Item = TextSlot<'a>>,
    member_sizes: &[u32],
    user_gap: u32,
    (text_base, loops): (u32, &UserLoops),
) -> TextPlacement {
    let keep_loops = std::env::var_os("TORAJS_LINK_LOOP_PLACE_OFF").is_none();
    let mut loop_pads = (0u32, 0u32);
    let mut place = |cursor: &mut u32, slot: TextSlot<'_>| {
        let mut at = cursor.next_multiple_of(if slot.packed { 4 } else { TEXT_FN_ALIGN });
        // a function inside one region keeps every loop of it inside too
        let start = text_base + at;
        let crosses = slot.size > 0 && start / LOOP_REGION != (start + slot.size - 1) / LOOP_REGION;
        // names are the link's symbols, one definition each; at most
        // one function crosses each boundary, so this runs that often
        if keep_loops
            && crosses
            && let Some((_, l)) = loops.iter().find(|(name, _)| name == slot.name)
        {
            let pad = loop_pad(start, l);
            if pad > 0 {
                loop_pads = (loop_pads.0 + 1, loop_pads.1 + pad);
            }
            at += pad;
        }
        *cursor = at + slot.size;
        at
    };
    let mut cursor: u32 = 0;
    let members = member_sizes
        .iter()
        .map(|&size| {
            let at = cursor.next_multiple_of(TEXT_FN_ALIGN);
            cursor = at + size;
            at
        })
        .collect();
    if user_gap > 0 && !member_sizes.is_empty() {
        cursor = cursor.next_multiple_of(TEXT_FN_ALIGN) + user_gap;
    }
    let user = user_slots.map(|s| place(&mut cursor, s)).collect();
    TextPlacement {
        user,
        members,
        size: cursor,
        loop_pads,
    }
}

/// The fetch lines to leave in front of a function at address `at`
/// so that its `loops`, shortest first, sit inside one
/// [`LOOP_REGION`]: the smallest pad that leaves none straddling, else
/// the smallest that keeps the first loop whole if any pad does, then
/// the second, and so on (the loops' straddling flags compared in
/// order, a pad keeping a loop beating every pad that does not). Only
/// pads up to the longest loop straddling at `at`, rounded up to a
/// fetch line, are tried.
fn loop_pad(at: u32, loops: &[(u32, u32)]) -> u32 {
    let straddles = |start: u32, &(lo, hi): &(u32, u32)| {
        hi > lo
            && hi - lo <= LOOP_REGION
            && (start + lo) / LOOP_REGION != (start + hi - 1) / LOOP_REGION
    };
    let flags = |start: u32| loops.iter().map(move |l| straddles(start, l));
    let cap = (loops.iter())
        .filter(|l| straddles(at, l))
        .map(|&(lo, hi)| (hi - lo).next_multiple_of(TEXT_FN_ALIGN))
        .max()
        .unwrap_or(0);
    let mut best = 0;
    let mut pad = TEXT_FN_ALIGN;
    while flags(at + best).any(|s| s) && pad <= cap {
        if flags(at + pad).lt(flags(at + best)) {
            best = pad;
        }
        pad += TEXT_FN_ALIGN;
    }
    best
}

pub(crate) fn write_text_payload(
    buf: &mut Vec<u8>,
    layout: &ArchiveLayout,
    resolved: &[ResolvedFunction],
    members: &MemberText<'_, '_>,
) -> Result<(), ArchiveLayoutError> {
    for (i, m) in layout.member_layouts.iter().enumerate() {
        pad_to(buf, m.file_offset as usize);
        members.write_text(buf, layout, i)?;
        debug_assert_eq!(
            buf.len() as u32,
            m.file_offset + m.text_size,
            "patched member __text size must match layout",
        );
    }
    let text_vmaddr = layout.text_vmaddr;
    for (r, &vaddr) in resolved.iter().zip(&layout.fn_vaddrs) {
        pad_to(buf, (vaddr - text_vmaddr) as usize);
        buf.extend_from_slice(&r.bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_start_is_aligned_and_nothing_overlaps() {
        let p = place_text(plain([4, 72, 64]), &[8, 100], 0, (0, &[]));
        assert_eq!(p.members, vec![0, 64]);
        assert_eq!(p.user, vec![192, 256, 384]);
        assert_eq!(p.size, 448);
    }

    /// The members do not move when the user code grows.
    #[test]
    fn the_members_ignore_the_user_code() {
        let short = place_text(plain([4]), &[8, 100], 0, (0, &[]));
        let long = place_text(plain([4, 4000]), &[8, 100], 0, (0, &[]));
        assert_eq!(short.members, long.members);
    }

    /// The gap moves every user function by the same whole fetch lines
    /// and leaves the members where they were.
    #[test]
    fn a_user_gap_moves_only_the_user_functions() {
        let p = place_text(plain([4, 72, 64]), &[8, 100], 128, (0, &[]));
        assert_eq!(p.members, vec![0, 64]);
        assert_eq!(p.user, vec![320, 384, 512]);
        assert_eq!(p.size, 576);
    }

    /// Reject stubs pack on 4 bytes; whatever follows them still opens
    /// a fetch line.
    #[test]
    fn reject_stubs_pack_and_the_next_function_realigns() {
        let p = place_text(
            [(72, false), (8, true), (8, true), (12, false)]
                .into_iter()
                .map(|(size, packed)| TextSlot {
                    size,
                    packed,
                    name: "",
                }),
            &[8],
            0,
            (0, &[]),
        );
        assert_eq!(p.members, vec![0]);
        assert_eq!(p.user, vec![64, 136, 144, 192]);
        assert_eq!(p.size, 204);
    }

    /// Unpacked slots of the given sizes, without loops.
    fn plain<const N: usize>(sizes: [u32; N]) -> impl Iterator<Item = TextSlot<'static>> {
        sizes.into_iter().map(|size| TextSlot {
            size,
            packed: false,
            name: "",
        })
    }

    fn fn_with(bytes: usize, target: &str, at: u32) -> CompiledFunction {
        CompiledFunction {
            name: "___torajs_dispatch_str_arm".into(),
            bytes: vec![0; bytes],
            relocs: vec![torajs_codegen::reloc::Reloc {
                byte_offset: at,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(target.into()),
                },
            }],
            frame: torajs_codegen::frame::FrameLayout::leaf_no_spill(),
        }
    }

    /// Only the exact stub shape packs: eight bytes, one tail branch
    /// into a reject landing pad.
    #[test]
    fn only_a_reject_stub_packs() {
        let pad = "___torajs_dispatch_stub_reject";
        let slot = |f: &CompiledFunction| (text_slot(f).size, text_slot(f).packed);
        assert_eq!(slot(&fn_with(8, pad, 4)), (8, true));
        assert_eq!(
            slot(&fn_with(8, "___torajs_dispatch_stub_reject_ptr", 4)),
            (8, true)
        );
        assert_eq!(slot(&fn_with(12, pad, 8)), (12, false));
        assert_eq!(slot(&fn_with(8, "___torajs_rc_dec", 4)), (8, false));
        assert_eq!(slot(&fn_with(8, pad, 0)), (8, false));
    }

    /// The answer-0 stub packs only with its exact bytes and no reloc.
    #[test]
    fn an_answer_zero_stub_packs() {
        let mut f = fn_with(8, "___torajs_rc_dec", 4);
        f.bytes = ANSWER_ZERO_STUB.to_vec();
        assert!(!text_slot(&f).packed);
        f.relocs.clear();
        assert!(text_slot(&f).packed);
        f.bytes[0] = 0x20;
        assert!(!text_slot(&f).packed);
    }

    #[test]
    fn empty_layout_has_zero_size() {
        let p = place_text(std::iter::empty(), &[], 0, (0, &[]));
        assert!(p.user.is_empty() && p.members.is_empty());
        assert_eq!(p.size, 0);
    }

    /// A function whose loop would straddle the region boundary moves
    /// down by whole fetch lines until the loop sits inside one; a
    /// loop already inside costs nothing, and so does one too long to
    /// fit any region.
    #[test]
    fn a_loop_is_kept_inside_one_region() {
        // the loop at [0x80, 0x180) from a start of 0xf00 crosses 0x1000
        assert_eq!(loop_pad(0xf00, &[(0x80, 0x180)]), 0x80);
        assert_eq!(loop_pad(0xf00, &[(0x80, 0x140)]), 0x80);
        assert_eq!(loop_pad(0xe00, &[(0x100, 0x200)]), 0);
        assert_eq!(loop_pad(0xf00, &[(0, 0x100)]), 0);
        assert_eq!(loop_pad(0xf00, &[(0x80, 0x80 + LOOP_REGION + 4)]), 0);
        // two loops: the smallest pad keeping both whole
        assert_eq!(loop_pad(0xf00, &[(0x80, 0x180), (0, 0x300)]), 0x100);
        // the pad never passes the longest straddling loop, rounded up
        // to a fetch line, even when a longer one would keep more
        for at in (0xe00..0x1000).step_by(4) {
            let loops = [(0x10, 0x90), (0x200, 0x2c4)];
            assert!(loop_pad(at, &loops) <= 0x100, "{at:#x}");
        }
        let loops = [("f".to_string(), vec![(0x100, 0x200)])];
        // the stats count the one function moved and its pad
        let p = place_text(
            [TextSlot {
                size: 0x300,
                packed: false,
                name: "f",
            }]
            .into_iter(),
            &[0x40],
            0,
            (0xe80, &loops),
        );
        // the member at 0; the function would start at 0x40 (address 0xec0,
        // loop 0xfc0..0x10c0) and moves to 0x80 (address 0xf00, loop 0x1000..)
        assert_eq!(p.user, vec![0x80]);
        assert_eq!(p.size, 0x380);
        assert_eq!(p.loop_pads, (1, 0x40));
    }

    /// Past 64 loops the order still decides: seventy all straddling
    /// move together, and with 64 short loops in front the 65th wins
    /// over the 66th.
    #[test]
    fn more_than_64_loops_keep_their_order() {
        let same = vec![(0x80, 0x180); 70];
        assert_eq!(loop_pad(0xf00, &same), 0x80);
        // at 0xf00 the 65th straddles and the 66th does not; from 0x80
        // on the other way round; the 64 in front never straddle
        let mut loops = vec![(0, 4); 64];
        loops.extend([(0x80, 0x140), (0, 0xd0)]);
        assert_eq!(loop_pad(0xf00, &loops), 0x80);
        loops.swap(64, 65);
        assert_eq!(loop_pad(0xf00, &loops), 0);
    }
}
