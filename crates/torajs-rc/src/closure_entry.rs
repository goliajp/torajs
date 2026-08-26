//! The one place the runtime reads a closure cell's boxed dual entry
//! (RFC 20260824-s2-5 刀 4 A1, link-judged form).
//!
//! Every lifted closure gets a synthesized any-ABI adapter
//! (`__boxed_<name>(env, argv, argc) -> AnyValue`) whose address the
//! mint stores at `boxed_entry@32`. The adapter's per-parameter unbox
//! is one reloc away from the whole any world, so a program with one
//! directly-called closure linked 397 KB against 84 KB without it —
//! and the store is unconditional because the compiler cannot see
//! whether the cell will ever be invoked dynamically (a closure value
//! crosses into the any world as a `Ptr`, not a `Closure`; r500's SSA
//! escape judgment misjudged `a.sort(fn)` and object-literal methods
//! and was reverted).
//!
//! The link can see it. A dynamic invocation always reads the slot,
//! and after this file every such read in the runtime goes through
//! [`__torajs_closure_boxed_entry`] — `#[inline(never)]` and exported
//! under a C name, so from any other archive member it is a `bl` to
//! this symbol and its text is live exactly when some live code can
//! invoke a closure through its boxed entry. `torajs-link`'s
//! dead-strip pre-pass uses that liveness as the guard for the mint
//! sites' `adrp/add` pairs: when the symbol's text is dead the pair
//! becomes `movz Xd, #0` (the pre-existing no-adapter shape) and the
//! orphaned adapter is stripped. Inlining cannot defeat the evidence
//! (the call crosses a crate boundary), and a reader added later that
//! bypasses this entry would read the 0 and answer a catchable
//! TypeError — loud, never a mis-ABI call.
//!
//! Identity probes that compare the slot against a runtime-minted
//! entry (`bare_entry` / `class_bare_entry` in torajs-anyvalue) keep
//! their raw read: a 0 never equals a runtime entry, so the answer is
//! the same whether the adapter was linked or not, and their liveness
//! must not root the adapters.

/// Closure-cell boxed dual-entry slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_BOXED_ENTRY_OFF`.
pub const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// Read `cell`'s boxed dual entry (0 = no adapter linked).
///
/// # Safety
/// `cell` is a live `Tag::Closure` heap cell.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_boxed_entry(cell: *const u8) -> u64 {
    unsafe { *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) }
}
