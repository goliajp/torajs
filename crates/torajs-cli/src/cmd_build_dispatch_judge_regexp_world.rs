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

use torajs_core::ssa::{InstKind, Module, Operand};

/// %RegExp.prototype%'s builtin tag — also the RegExp ctor cell's and
/// the RegExp method family's.
const REGEXP_TAG: i64 = torajs_rc::builtin_proto::REGEXP_PROTO_TAG as i64;

/// The RegExp protocol method ids — the `@@match` family the RegExp
/// arm hands to the protocol seam.
const PROTOCOL_MIDS: [i64; 5] = [
    torajs_rc::ANY_METHOD_MATCH,
    torajs_rc::ANY_METHOD_MATCH_ALL,
    torajs_rc::ANY_METHOD_REPLACE,
    torajs_rc::ANY_METHOD_SEARCH,
    torajs_rc::ANY_METHOD_SPLIT,
];

/// Whether a call with `mid` can enter the protocol seam: the five
/// protocol ids on a RegExp receiver, and on a string receiver those
/// five and `replaceAll`, whose step 2 calls a RegExp pattern's
/// `@@replace` (the seam answers a plain one the kernel lane).
pub(crate) fn enters_protocol(mid: i64) -> bool {
    PROTOCOL_MIDS.contains(&mid) || mid == torajs_rc::ANY_METHOD_REPLACE_ALL
}

/// The method ids whose protocol call needs the algorithms even over a
/// plain RegExp: @@matchAll answers an iterator no kernel builds,
/// @@split's `limit` may be an object (whose `valueOf` runs after the
/// splitter copied the receiver), and `compile` recompiles a receiver
/// in place — from inside such a `valueOf` too.
pub(crate) fn unplain_mid(mid: i64) -> bool {
    mid == torajs_rc::ANY_METHOD_MATCH_ALL
        || mid == torajs_rc::ANY_METHOD_SPLIT
        || mid == torajs_rc::ANY_METHOD_COMPILE
}

/// Calls that can leave a RegExp cell or %RegExp.prototype% carrying
/// something the protocol algorithms observe and the native kernels do
/// not — an own property, a locked or boxed `lastIndex`, a prototype
/// other than %RegExp.prototype%, a subclass, a recompiled matcher —
/// or hand a RegExp-family method cell to a non-RegExp object: the any
/// world's writers, property definition, integrity levels, Reflect,
/// prototype reads and writes, `Object.create`, a RegExp subclass, a
/// typed `compile`, and any reified namespace static (whose call
/// surface is the whole of `Object` / `Reflect`). Not the typed field
/// write's slow leg: its receiver is a class instance, and a method
/// cell it stores is called with that `this` only by an id the scan
/// observes or one [`crate::cmd_build_dispatch_judge_regexp_unseen`]
/// rules out.
const UNPLAIN_PREFIXES: [&str; 16] = [
    "__torajs_any_member_set",
    "__torajs_any_index_set",
    "__torajs_dynobj_dict_set",
    "__torajs_anyv_proto_member_set",
    "__torajs_anyv_super_prop_set",
    "__torajs_dynobj_define",
    "__torajs_anyv_define",
    "__torajs_anyv_assign",
    "__torajs_anyv_freeze",
    "__torajs_obj_freeze",
    "__torajs_anyv_seal",
    "__torajs_anyv_prevent_extensions",
    "__torajs_anyv_reflect_",
    "__torajs_reflect_",
    "__torajs_ns_",
    "__torajs_object_create",
];

/// [`UNPLAIN_PREFIXES`] plus the prototype and RegExp-specific ones.
fn makes_unplain(name: &str) -> bool {
    UNPLAIN_PREFIXES.iter().any(|p| name.starts_with(p))
        || name.starts_with("__torajs_anyv_set_prototype_of")
        || name.starts_with("__torajs_anyv_get_proto_of")
        || name.starts_with("__torajs_regex_subclass")
        || name == "__torajs_regex_compile_inplace"
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

/// Whether a call to a callee can hand out the RegExp constructor or
/// %RegExp.prototype% (and so opens the world): a runtime-keyed view
/// of the global object always, an entry that mints a builtin ctor /
/// prototype / method cell from a tag operand when that operand is
/// RegExp's or not a constant.
#[derive(Clone, Copy)]
pub(crate) enum CtorHandout {
    Never,
    Always,
    /// The operand index of the tag.
    TagAt(usize),
}

impl CtorHandout {
    pub(crate) fn of(name: &str) -> Self {
        match name {
            "__torajs_globalthis_object" | "__torajs_global_eval_value" => Self::Always,
            "__torajs_builtin_ctor_value"
            | "__torajs_get_builtin_prototype"
            | "__torajs_builtin_proto_method_value"
            | "__torajs_builtin_method_cell_tagged" => Self::TagAt(0),
            "__torajs_proto_chain_builtin" => Self::TagAt(1),
            "__torajs_anyv_class_register" => Self::TagAt(4),
            _ => Self::Never,
        }
    }

    /// Whether the call with `args` hands one out.
    pub(crate) fn hands_out(self, args: &[Operand]) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::TagAt(i) => {
                !matches!(args.get(i), Some(Operand::ConstI64(t)) if *t != REGEXP_TAG)
            }
        }
    }
}

/// What the judgment scan saw of the RegExp world.
pub(crate) struct RegexpWorld {
    /// Any way into the world.
    pub(crate) world: bool,
    /// A call some [`CtorHandout`] hands one out.
    pub(crate) ctor: bool,
    /// The judgment's own constructor-read verdict, when it computed
    /// one (`Some(true)` also for any other ns-world entry).
    pub(crate) ctor_read: Option<bool>,
    /// A way into the protocol seam other than from inside it: a
    /// protocol method id, the string symbol-dispatch gate, or a
    /// fallback trigger that can hide one
    /// ([`crate::cmd_build_dispatch_judge_regexp_unseen`]).
    pub(crate) protocol_entry: bool,
    /// No hidden protocol id, no string symbol-dispatch gate (whose
    /// pattern may be any object carrying a RegExp-family method
    /// cell) and no [`unplain_mid`].
    pub(crate) plain_candidate: bool,
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
    /// The algorithms behind the protocol seam: with it, and alone when
    /// every protocol call the program makes is over a plain RegExp
    /// ([`RegexpWorld::all_plain`]) — the seam's own lane judgment then
    /// answers the kernels' lane, and a call it would send to the
    /// algorithms reaches the loud stub instead.
    pub(crate) algorithms: bool,
    pub(crate) ctor: bool,
}

impl RegexpWorld {
    pub(crate) fn verdict(&self, module: &Module, diag: bool) -> RegexpVerdict {
        let (arm_bits, protocol, ctor) = if !self.world {
            (torajs_rc::any_method_family::FAM_REGEXP, true, true)
        } else if self.ctor || self.protocol_entry {
            // all plain: the algorithms, the only constructor of a
            // RegExp through the build seam besides a ctor value, are
            // stubbed too
            let plain = self.all_plain(module, diag);
            return RegexpVerdict {
                arm_bits: 0,
                protocol: !self.protocol_entry,
                algorithms: !self.protocol_entry || plain,
                ctor: plain,
            };
        } else {
            let ctor_read = self.ctor_read.unwrap_or_else(|| {
                crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable(module, diag)
            });
            (0, true, !ctor_read)
        };
        RegexpVerdict {
            arm_bits,
            protocol,
            algorithms: protocol,
            ctor,
        }
    }

    /// Whether every RegExp a protocol call can meet stays plain (no
    /// own property, no locked or boxed `lastIndex`, no subclass, an
    /// untouched %RegExp.prototype% and `RegExp[@@species]`) and every
    /// call is one a plain receiver answers through the kernels: no
    /// way to the constructor or the prototype, and no call that could
    /// change either or the cell ([`makes_unplain`]). The second walk
    /// runs only for a candidate.
    fn all_plain(&self, module: &Module, diag: bool) -> bool {
        if self.ctor || !self.plain_candidate {
            return false;
        }
        let ctor_read = self.ctor_read.unwrap_or_else(|| {
            crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable(module, diag)
        });
        let unplain_call = module
            .funcs
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.insts)
            .find_map(|i| match &i.kind {
                InstKind::Call(fid, _) => Some(module.funcs[fid.0 as usize].name.as_str())
                    .filter(|name| makes_unplain(name)),
                _ => None,
            });
        if diag {
            eprintln!(
                "[judge] regexp all-plain: {} (ctor read {ctor_read}, unplain call {unplain_call:?})",
                !ctor_read && unplain_call.is_none()
            );
        }
        !ctor_read && unplain_call.is_none()
    }
}
