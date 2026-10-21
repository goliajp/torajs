//! How a callee can hand the program the `@@match` symbol — one of the
//! RegExp world's inputs ([`crate::cmd_build_dispatch_judge_regexp_world`]).

use torajs_core::ssa::Operand;

use crate::cmd_build_dispatch_judge_regexp_world::{CtorHandout, REGEXP_TAG};

/// How a callee can hand the program the `@@match` symbol, which is
/// what lets an Object that is no RegExp answer §22.1.1.10 IsRegExp
/// yes (String.prototype.matchAll / replaceAll step 2.a). Without it
/// no such Object exists, and outside the RegExp world the protocol
/// seam's step 2 ask needs no body.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MatchSymbolSource {
    None,
    /// `__torajs_symbol_well_known`: `@@match` itself unless the
    /// index operand names another symbol.
    WellKnown,
    /// Mints a symbol of another kind, which a `constructor` read
    /// turns into the Symbol constructor.
    Mint,
    /// A proxy (its traps receive keys), or a builtin constructor,
    /// prototype or method cell handed out from a tag operand.
    Handout,
}

/// `WELL_KNOWN_SYMBOL_NAMES` index of `match`.
const WK_MATCH: i64 = 6;

/// %Symbol.prototype%'s builtin tag (torajs-rc's prototype slot order).
const SYMBOL_TAG: i64 = 5;

impl MatchSymbolSource {
    pub(crate) fn of(name: &str, handout: CtorHandout) -> Self {
        match name {
            "__torajs_symbol_well_known" => Self::WellKnown,
            "__torajs_symbol_iterator"
            | "__torajs_symbol_async_iterator"
            | "__torajs_symbol_to_primitive"
            | "__torajs_symbol_for"
            | "__torajs_symbol_for_any"
            | "__torajs_symbol_alloc" => Self::Mint,
            _ if name.starts_with("__torajs_proxy") || !matches!(handout, CtorHandout::Never) => {
                Self::Handout
            }
            _ => Self::None,
        }
    }

    /// Fold one call with `args` into `seen` (direct, minted).
    pub(crate) fn observe(self, handout: CtorHandout, args: &[Operand], seen: &mut (bool, bool)) {
        let not_tag =
            |i: usize, tag: i64| matches!(args.get(i), Some(Operand::ConstI64(t)) if *t != tag);
        match self {
            Self::None => {}
            Self::WellKnown => {
                seen.0 |= !not_tag(0, WK_MATCH);
                seen.1 = true;
            }
            Self::Mint => seen.1 = true,
            Self::Handout => {
                seen.0 |= match handout {
                    CtorHandout::TagAt(i) => !(not_tag(i, REGEXP_TAG) && not_tag(i, SYMBOL_TAG)),
                    _ => true,
                };
            }
        }
    }
}
