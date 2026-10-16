//! Whether a program can ever hold a RegExp cell, %RegExp.prototype%
//! or the RegExp constructor — the RegExp world — and whether it can
//! hold the constructor itself.
//!
//! The RegExp family arm and two seams are reachable only from inside
//! that world: the arm itself (entered for a RegExp receiver, for an
//! object `this` handed a RegExp-family method cell, which only
//! %RegExp.prototype% carries, and for a RegExp String Iterator
//! step), the protocol algorithms behind it, and the build body a
//! first-class RegExp ctor value runs. Outside it all three get
//! loud-reject stubs, whatever else the dispatch judgment concludes:
//! a fallback program re-dispatches arbitrary method ids, but only
//! over values that exist.
//!
//! The scan names every compiler-emitted way into the world:
//!
//! - the regex family's own entries (a literal, `new RegExp`, a typed
//!   `RegExp.x` static, a subclass) and the string kernels that take a
//!   RegExp cell ([`mints_regexp`]); not the string kernels that only
//!   share the family's dispatch prefixes (a string-pattern replace,
//!   whose pattern is a string or an any value some other entry
//!   minted);
//! - a builtin constructor, prototype or method cell minted from a
//!   compile-time tag, when the tag is RegExp's or not a constant: the
//!   bare `RegExp` ident as a value, `RegExp.prototype` and its
//!   members, a class whose heritage is RegExp;
//! - globalThis and the eval function as values, which hand out
//!   every global binding by a runtime key.
//!
//! Everything else that reaches a builtin constructor at run time
//! (`x.constructor`, a prototype read, Reflect, a proxy) reads it off
//! a value it already holds, and the RegExp ones only off a value of
//! the world.
//!
//! Inside the world the ctor build seam still stubs when the program
//! cannot hold the constructor: no entry of the second and third kind,
//! no `constructor` read off a value
//! ([`crate::cmd_build_dispatch_judge_ctor_reach`]), and a stubbed
//! protocol seam — the algorithms read `constructor` and construct
//! through it (SpeciesConstructor).

use torajs_core::ssa::{Module, Operand};

/// %RegExp.prototype%'s builtin tag — also the RegExp ctor cell's and
/// the RegExp method family's.
const REGEXP_TAG: i64 = 7;

/// The RegExp protocol method ids — the `@@match` family the RegExp
/// arm hands to the protocol seam.
const PROTOCOL_MIDS: [i64; 5] = [
    torajs_rc::ANY_METHOD_MATCH,
    torajs_rc::ANY_METHOD_MATCH_ALL,
    torajs_rc::ANY_METHOD_REPLACE,
    torajs_rc::ANY_METHOD_SEARCH,
    torajs_rc::ANY_METHOD_SPLIT,
];

pub(crate) fn is_protocol_mid(mid: i64) -> bool {
    PROTOCOL_MIDS.contains(&mid)
}

/// `s.match(x)` / `s.search(x)` / … over an any-typed pattern — the
/// gate calls the protocol seam when the pattern carries the builtin
/// method but is not a plain RegExp.
pub(crate) const STR_SYMBOL_GATE: &str = "__torajs_any_str_symbol_try";

/// True when a call to `name` mints a RegExp cell or takes one: the
/// `__torajs_regex*` / `__torajs_regexp*` entries, and a string kernel
/// whose pattern slot is a typed RegExp (`__torajs_str_*regex*`).
pub(crate) fn mints_regexp(name: &str) -> bool {
    name.starts_with("__torajs_regex")
        || (name.starts_with("__torajs_str_") && name.contains("regex"))
}

/// True when the call to `name` with `args` can hand out the RegExp
/// constructor or %RegExp.prototype% (and so opens the world): a
/// runtime-keyed view of the global object, or an entry that mints a
/// builtin ctor / prototype / method cell from a tag operand that is
/// RegExp's or not a constant. One `match` on the name: this runs for
/// every call the judgment scans.
pub(crate) fn hands_out_regexp_ctor(name: &str, args: &[Operand]) -> bool {
    let tag_at = match name {
        "__torajs_globalthis_object" | "__torajs_global_eval_value" => return true,
        "__torajs_builtin_ctor_value"
        | "__torajs_get_builtin_prototype"
        | "__torajs_builtin_proto_method_value"
        | "__torajs_builtin_method_cell_tagged" => 0,
        "__torajs_proto_chain_builtin" => 1,
        "__torajs_anyv_class_register" => 4,
        _ => return false,
    };
    !matches!(args.get(tag_at), Some(Operand::ConstI64(t)) if *t != REGEXP_TAG)
}

/// What the judgment scan saw of the RegExp world.
pub(crate) struct RegexpWorld {
    /// Any way into the world.
    pub(crate) world: bool,
    /// A [`hands_out_regexp_ctor`] entry.
    pub(crate) ctor: bool,
    /// The judgment's own constructor-read verdict, when it computed
    /// one (`Some(true)` also for any other ns-world entry).
    pub(crate) ctor_read: Option<bool>,
    /// A way into the protocol seam other than from inside it: a
    /// protocol method id, the string symbol-dispatch gate, or a
    /// fallback trigger (whose re-dispatched ids are unknown).
    pub(crate) protocol_entry: bool,
}

/// The stubs the RegExp world's judgment adds to the dispatch
/// judgment's own.
pub(crate) struct RegexpVerdict {
    /// The RegExp family arm bit when the program never enters the
    /// world: the arm is entered for a RegExp receiver, for an object
    /// `this` of a RegExp-family method cell and for a RegExp String
    /// Iterator step, and none of those exists outside it.
    pub(crate) arm_bits: u16,
    pub(crate) protocol: bool,
    pub(crate) ctor: bool,
}

impl RegexpWorld {
    pub(crate) fn verdict(&self, module: &Module, diag: bool) -> RegexpVerdict {
        let (arm_bits, protocol, ctor) = if !self.world {
            (torajs_rc::any_method_family::FAM_REGEXP, true, true)
        } else if self.ctor || self.protocol_entry {
            (0, !self.protocol_entry, false)
        } else {
            let ctor_read = self.ctor_read.unwrap_or_else(|| {
                crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable(module, diag)
            });
            (0, true, !ctor_read)
        };
        RegexpVerdict {
            arm_bits,
            protocol,
            ctor,
        }
    }
}
