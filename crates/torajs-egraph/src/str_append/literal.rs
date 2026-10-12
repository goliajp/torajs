//! Short static operands by value — a kernel call whose operand is a
//! static string of at most eight payload bytes takes that string as
//! two immediates instead of a pointer to its cell:
//!
//! ```text
//! %l = static_str_ref @","
//! %r = call __torajs_str_append(%acc, %l)
//! ```
//!
//! becomes
//!
//! ```text
//! %r = call __torajs_str_append_lit(%acc, 0x2c, 0x1012)
//! ```
//!
//! The literal kernels know what the general ones have to find out on
//! every call: the operand is not null, and its length, width and
//! ASCII-ness are the constants in the second immediate. Its bytes are
//! the first immediate, written with a store. The other operand is
//! untouched — a null there still reads as "null", and the result
//! still takes the wider of the two encodings.
//!
//! The rewrite is one call for one call over the same left or right
//! cell, so ownership does not move: a borrowing kernel maps to a
//! borrowing form and `append` to the consuming one. A static left
//! operand of `append` has no reference to hand over (its release is a
//! no-op), so the borrowing prefix form stands in for it.
//!
//! Runs after the pair and chain rewrites: those decide which kernel a
//! piece goes through, this only changes how a literal piece travels.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, InstKind, Module, Operand, StringId, StringLiteral, ValueId,
    visit_value_operands,
};

use super::kernels::{Kernels, Kind};

// the header flag bits a literal's cell carries (`torajs-str` layout)
pub(super) const STR_FLAG_IS_LATIN1: u64 = 1 << 1;
const STR_FLAG_IS_ASCII: u64 = 1 << 12;
/// Bit position of the code-unit count in the second immediate; the
/// flag bits sit at their header positions around it.
const LEN_SHIFT: u32 = 4;
const MAX_PAYLOAD_BYTES: usize = 8;

const CONCAT_NAME: &str = "__torajs_str_concat_lit";
const PREPEND_NAME: &str = "__torajs_str_prepend_lit";
const APPEND_NAME: &str = "__torajs_str_append_lit";
const SUBSTR_CONCAT_NAME: &str = "__torajs_substr_concat_lit";
const SUBSTR_PREPEND_NAME: &str = "__torajs_substr_prepend_lit";

/// The `(bits, meta)` immediates of a literal short enough to travel
/// by value.
pub(crate) fn immediates(lit: &StringLiteral) -> Option<(i64, i64)> {
    if lit.bytes.is_empty() || lit.bytes.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    let mut raw = [0u8; MAX_PAYLOAD_BYTES];
    raw[..lit.bytes.len()].copy_from_slice(&lit.bytes);
    let mut meta = (lit.length as u64) << LEN_SHIFT;
    if lit.is_latin1 {
        meta |= STR_FLAG_IS_LATIN1;
        if lit.bytes.iter().all(|b| *b < 0x80) {
            meta |= STR_FLAG_IS_ASCII;
        }
    }
    Some((u64::from_le_bytes(raw) as i64, meta as i64))
}

/// The literal kernels a module declares and the immediates of each of
/// its short strings.
pub(super) struct LiteralForms {
    concat: Option<FuncId>,
    prepend: Option<FuncId>,
    append: Option<FuncId>,
    substr_concat: Option<FuncId>,
    substr_prepend: Option<FuncId>,
    /// Indexed by `StringId`.
    immediates: Vec<Option<(i64, i64)>>,
}

impl LiteralForms {
    pub fn find(module: &Module) -> Self {
        let find = |name: &str| {
            module
                .funcs
                .iter()
                .position(|f| f.name == name)
                .map(|i| FuncId(i as u32))
        };
        Self {
            concat: find(CONCAT_NAME),
            prepend: find(PREPEND_NAME),
            append: find(APPEND_NAME),
            substr_concat: find(SUBSTR_CONCAT_NAME),
            substr_prepend: find(SUBSTR_PREPEND_NAME),
            immediates: module.strings.iter().map(immediates).collect(),
        }
    }

    fn immediates_of(
        &self,
        op: &Operand,
        statics: &HashMap<ValueId, StringId>,
    ) -> Option<(ValueId, (i64, i64))> {
        let Operand::Value(v) = op else { return None };
        let sid = statics.get(v)?;
        Some((*v, (*self.immediates.get(sid.0 as usize)?)?))
    }

    /// Rewrite every kernel call in `func` that has a short static
    /// operand; answers how many.
    pub fn apply(
        &self,
        func: &mut Function,
        kernels: &Kernels,
        statics: &HashMap<ValueId, StringId>,
    ) -> u32 {
        let mut rewritten = 0;
        let mut moved: HashSet<ValueId> = HashSet::default();
        for inst in func.blocks.iter_mut().flat_map(|b| b.insts.iter_mut()) {
            let Some((k, args)) = kernels.call(inst) else {
                continue;
            };
            let right = (k.right == Kind::Str)
                .then(|| self.immediates_of(&args[1], statics))
                .flatten();
            let left = (k.left == Kind::Str)
                .then(|| self.immediates_of(&args[0], statics))
                .flatten();
            let plan = if let Some((v, imm)) = right {
                let form = match (k.left, k.consumes_left) {
                    (Kind::Str, false) => self.concat,
                    (Kind::Str, true) => self.append,
                    (Kind::Substr, _) => self.substr_concat,
                };
                form.map(|fid| (fid, args[0], v, imm))
            } else if let Some((v, imm)) = left {
                let form = match k.right {
                    Kind::Str => self.prepend,
                    Kind::Substr => self.substr_prepend,
                };
                form.map(|fid| (fid, args[1], v, imm))
            } else {
                None
            };
            let Some((fid, other, literal, (bits, meta))) = plan else {
                continue;
            };
            inst.kind = InstKind::Call(
                fid,
                vec![other, Operand::ConstI64(bits), Operand::ConstI64(meta)],
            );
            moved.insert(literal);
            rewritten += 1;
        }
        if !moved.is_empty() {
            drop_unread(func, moved);
        }
        rewritten
    }
}

/// Delete the `static_str_ref` of every value in `moved` that nothing
/// reads any more.
pub(crate) fn drop_unread(func: &mut Function, mut moved: HashSet<ValueId>) {
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                moved.remove(&v);
            });
        }
        super::visit_terminator_values(&block.term, |v| {
            moved.remove(&v);
        });
    }
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| {
            !(matches!(inst.kind, InstKind::StaticStrRef(_))
                && inst.result.is_some_and(|r| moved.contains(&r)))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::rewrite_str_appends;
    use super::super::test_support::*;
    use super::*;
    use torajs_core::ssa::{Inst, Type};

    /// `@0` = ",", `@1` = "中文", `@2` = nine bytes, `@3` = "",
    /// `@4` = "é".
    fn strings() -> Vec<StringLiteral> {
        vec![
            StringLiteral::encode_from_str(","),
            StringLiteral::encode_from_str("\u{4E2D}\u{6587}"),
            StringLiteral::encode_from_str("123456789"),
            StringLiteral::encode_from_str(""),
            StringLiteral::encode_from_str("\u{E9}"),
        ]
    }

    fn lit(result: u32, sid: u32) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::StaticStrRef(StringId(sid)),
            origin: None,
        }
    }

    fn run(insts: Vec<Inst>) -> (Module, super::super::StrAppendStats) {
        let mut m = module_with(insts);
        m.strings = strings();
        let stats = rewrite_str_appends(&mut m);
        (m, stats)
    }

    fn body(m: &Module) -> &[Inst] {
        &m.funcs[BODY].blocks[0].insts
    }

    fn call_of(inst: &Inst) -> (FuncId, Vec<Operand>) {
        match &inst.kind {
            InstKind::Call(f, args) => (*f, args.clone()),
            other => panic!("not a call: {other:?}"),
        }
    }

    const COMMA: i64 = 0x2C;
    const COMMA_META: i64 = (1 << LEN_SHIFT) | 0x1002;

    #[test]
    fn a_short_right_literal_of_a_borrowing_concat_goes_by_value() {
        let (m, stats) = run(vec![lit(10, 0), concat(4, 2, 10)]);
        assert_eq!(stats.by_value, 1);
        assert_eq!(
            body(&m).len(),
            1,
            "the static ref is gone with its only read"
        );
        assert_eq!(
            call_of(&body(&m)[0]),
            (
                STR_CONCAT_LIT,
                vec![
                    v(2),
                    Operand::ConstI64(COMMA),
                    Operand::ConstI64(COMMA_META)
                ]
            )
        );
        assert_eq!(body(&m)[0].result, Some(ValueId(4)));
    }

    #[test]
    fn a_consuming_append_maps_to_the_consuming_form() {
        // `s = s + ","`: the pair becomes an append first
        let (m, stats) = run(vec![lit(10, 0), concat(4, 2, 10), drop_of(2)]);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.by_value, 1);
        assert_eq!(body(&m).len(), 1);
        assert_eq!(call_of(&body(&m)[0]).0, STR_APPEND_LIT);
    }

    #[test]
    fn a_view_left_operand_takes_the_view_form() {
        let (m, stats) = run(vec![
            lit(10, 0),
            call(SUBSTR_CONCAT_SUBSTR_STR, 4, &[2, 10]),
        ]);
        assert_eq!(stats.by_value, 1);
        let (fid, args) = call_of(&body(&m)[0]);
        assert_eq!(fid, SUBSTR_CONCAT_LIT);
        assert_eq!(args[0], v(2));
    }

    #[test]
    fn a_left_literal_takes_the_prefix_form_over_the_right_operand() {
        let (m, stats) = run(vec![
            lit(10, 0),
            concat(4, 10, 2),
            call(SUBSTR_CONCAT_STR_SUBSTR, 5, &[10, 3]),
        ]);
        assert_eq!(stats.by_value, 2);
        assert_eq!(body(&m).len(), 2);
        assert_eq!(
            call_of(&body(&m)[0]),
            (
                STR_PREPEND_LIT,
                vec![
                    v(2),
                    Operand::ConstI64(COMMA),
                    Operand::ConstI64(COMMA_META)
                ]
            )
        );
        let (fid, args) = call_of(&body(&m)[1]);
        assert_eq!(fid, SUBSTR_PREPEND_LIT);
        assert_eq!(args[0], v(3));
    }

    #[test]
    fn a_utf16_literal_carries_its_units_and_no_flags() {
        let (m, stats) = run(vec![lit(10, 1), concat(4, 2, 10)]);
        assert_eq!(stats.by_value, 1);
        let (_, args) = call_of(&body(&m)[0]);
        assert_eq!(args[1], Operand::ConstI64(0x6587_4E2D));
        assert_eq!(args[2], Operand::ConstI64(2 << LEN_SHIFT));
    }

    #[test]
    fn a_non_ascii_latin1_literal_is_not_marked_ascii() {
        let (m, _) = run(vec![lit(10, 4), concat(4, 2, 10)]);
        let (_, args) = call_of(&body(&m)[0]);
        assert_eq!(args[1], Operand::ConstI64(0xE9));
        assert_eq!(args[2], Operand::ConstI64((1 << LEN_SHIFT) | 0x2));
    }

    #[test]
    fn a_long_or_empty_literal_keeps_its_cell() {
        for sid in [2, 3] {
            let (m, stats) = run(vec![lit(10, sid), concat(4, 2, 10)]);
            assert_eq!(stats.by_value, 0, "string {sid}");
            assert_eq!(call_of(&body(&m)[1]).0, STR_CONCAT);
        }
    }

    #[test]
    fn a_literal_with_another_reader_keeps_its_static_ref() {
        let (m, stats) = run(vec![lit(10, 0), concat(4, 2, 10), concat(5, 10, 10)]);
        // the second call reads the cell as its left operand
        assert_eq!(stats.by_value, 2);
        assert_eq!(body(&m).len(), 3);
        assert!(matches!(body(&m)[0].kind, InstKind::StaticStrRef(_)));
        assert_eq!(
            call_of(&body(&m)[2]),
            (
                STR_CONCAT_LIT,
                vec![
                    v(10),
                    Operand::ConstI64(COMMA),
                    Operand::ConstI64(COMMA_META)
                ]
            )
        );
    }

    #[test]
    fn a_value_with_a_second_definition_is_not_a_literal() {
        let copy = Inst {
            result: Some(ValueId(10)),
            kind: InstKind::Copy(Type::Str, v(3)),
            origin: None,
        };
        let (m, stats) = run(vec![lit(10, 0), copy, concat(4, 2, 10)]);
        assert_eq!(stats.by_value, 0);
        assert_eq!(call_of(&body(&m)[2]).0, STR_CONCAT);
    }

    #[test]
    fn a_module_without_the_literal_kernels_is_left_alone() {
        let mut m = module_with(vec![lit(10, 0), concat(4, 2, 10)]);
        m.strings = strings();
        for fid in [STR_CONCAT_LIT, STR_APPEND_LIT] {
            m.funcs[fid.0 as usize].name = "__torajs_something_else".into();
        }
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.by_value, 0);
        assert_eq!(body(&m).len(), 2);
    }

    #[test]
    fn the_flattened_chain_takes_its_literal_piece_by_value() {
        // `o = o + (v + "," + k)` with `v`, `k` views
        let (m, stats) = run(vec![
            lit(10, 0),
            call(SUBSTR_CONCAT_SUBSTR_STR, 11, &[1, 10]),
            call(SUBSTR_CONCAT_STR_SUBSTR, 12, &[11, 2]),
            drop_of(11),
            call(STR_APPEND, 13, &[3, 12]),
            drop_of(12),
        ]);
        assert_eq!(stats.flattened, 1);
        assert_eq!(stats.by_value, 1);
        let fids: Vec<FuncId> = body(&m).iter().map(|i| call_of(i).0).collect();
        assert_eq!(
            fids,
            vec![STR_APPEND_SUBSTR, STR_APPEND_LIT, STR_APPEND_SUBSTR]
        );
    }
}
