//! Keyed access in the throw scan: an index read's own raises, and
//! the non-integral key family. `xs[1.5]` names the property "1.5"
//! (§7.1.19), which a typed lane answers from the keyed read; whether
//! any such property can run code is settled program-wide
//! (`compute_may_throw_fns`), so the scan only records the reads and
//! the stores that can make one.

use super::Scan;
use crate::ast::{Expr, ExprId};
use crate::check::Type;

impl Scan<'_> {
    /// A keyed access whose key may not be an array index names a
    /// property (`xs[1.5]` names "1.5", §7.1.19), and the typed lanes'
    /// `__torajs_arr_typed_nonint_*` look it up and record any throw.
    /// An integer literal `lower_number` keeps I64 (|n| ≤ 2^53), or a
    /// name the width table keeps integral, lowers to the I64 index
    /// that never asks. Whether the lookup can raise is a whole-program
    /// answer (`compute_may_throw_fns`), so this only records the read.
    pub(super) fn key_may_name_property(&self, index: ExprId) -> bool {
        match self.ast.get_expr(index) {
            Expr::Number(n) => n.fract() != 0.0 || !n.is_finite() || n.abs() > 9.007199254740992e15,
            Expr::Ident(n) => {
                let keys = self.widths.ident_keys(&self.fn_name, n);
                !matches!(
                    self.expr_types.get(&index),
                    Some(crate::check::Type::Number)
                ) || keys.is_empty()
                    || keys.iter().any(|k| self.widths.slot_is_f64(k))
            }
            _ => true,
        }
    }

    /// A keyed store that can leave a named property holding a value
    /// whose ToNumber runs code on an array a typed read later asks:
    /// the key may not be an index, the value is not a number / string
    /// / boolean, and the receiver may hold an array. Its element type
    /// does not matter: a `number[]` store keeps a non-index key's
    /// value as it is (`xs[k] = v`, `v: any` holding an object), and a
    /// receiver that travels as `any` (a `Record` / index-signature
    /// alias, a union) reaches the same cell.
    pub(super) fn keyed_write_carries_code(&self, target: ExprId, value: ExprId) -> bool {
        let Expr::Index { obj, index } = self.ast.get_expr(target) else {
            return false;
        };
        let plain = matches!(
            self.lane_of(value),
            Some(Type::Number | Type::String | Type::Boolean)
        );
        !plain && may_hold_array(self.lane_of(*obj)) && self.key_may_name_property(*index)
    }

    /// The read `obj[index]` may ask for the property its key names.
    /// An array of any element type does for a key that may not be an
    /// array index (the typed lanes' `__torajs_arr_typed_nonint_*`, the
    /// keyed kernel behind `emit_any_elem_read_split`), as does a
    /// receiver typed `any` or not known. Where the program can put a
    /// property on an array's or a string's prototype chain
    /// (`Ast::array_prop_reads_may_raise`), every key may: an
    /// out-of-range index asks the chain (`emit_oob_named_read`), and a
    /// string asks for any key its view does not answer
    /// (`emit_str_keyed_read`).
    fn read_asks(&self, obj: ExprId, index: ExprId) -> bool {
        let chain = self.ast.array_prop_reads_may_raise;
        match self.lane_of(obj) {
            Some(Type::Array(_) | Type::Any | Type::TypeVar(_)) | None => {
                chain || self.key_may_name_property(index)
            }
            Some(Type::String) => chain,
            _ => false,
        }
    }

    /// The type `eid`'s value travels as, nullable or not.
    fn lane_of(&self, eid: ExprId) -> Option<Type> {
        match self.expr_types.get(&eid)?.clone().lane() {
            Type::Nullable(t) => Some(*t),
            t => Some(t),
        }
    }

    pub(super) fn index_read(&mut self, obj: ExprId, index: ExprId) {
        // Same as the Member arm: any-receiver index reads throw on
        // null/undefined via __torajs_any_index, typed ones through
        // the nullish guard in front of the element load.
        if matches!(self.expr_types.get(&obj), Some(crate::check::Type::Any))
            || self.receiver_raises(obj)
        {
            self.direct = true;
        }
        self.nonint_read |= self.read_asks(obj, index);
        self.expr(obj);
        self.expr(index);
    }

    pub(super) fn opt_index_read(&mut self, obj: ExprId, index: ExprId) {
        self.nonint_read |= self.read_asks(obj, index);
        self.expr(obj);
        self.expr(index);
    }
}

/// Whether a receiver of this type may be an array cell: an unknown
/// or `any`-lane type, an array, or an object type an `as any` cast
/// can put an array behind. Only the builtin kinds whose cells are
/// never arrays are ruled out.
fn may_hold_array(t: Option<Type>) -> bool {
    !matches!(
        t,
        Some(
            Type::Number
                | Type::String
                | Type::Boolean
                | Type::BigInt
                | Type::Symbol
                | Type::Void
                | Type::Never
                | Type::Null
                | Type::Undefined
                | Type::Function(..)
                | Type::Map(..)
                | Type::Set
                | Type::WeakRef
                | Type::WeakMap
                | Type::WeakSet
                | Type::MapIter(_)
                | Type::ArrIter
                | Type::RegExp
                | Type::Date
                | Type::Promise(_)
                | Type::Object(_)
        )
    )
}
