//! The shapes the inline element lowering builds, on hand-made
//! functions, and the layout facts it reads.

use super::*;
use crate::ssa::{ArrId, ArrLayouts};

/// sid 0 `{kind, value}`, sid 1 `{a, b, c}`, sid 2 `{s: string}`.
fn layouts() -> Vec<Vec<(PropKey, Type)>> {
    vec![
        vec![
            (PropKey::from("kind"), Type::F64),
            (PropKey::from("value"), Type::F64),
        ],
        vec![
            (PropKey::from("a"), Type::I64),
            (PropKey::from("b"), Type::F64),
            (PropKey::from("c"), Type::Bool),
        ],
        vec![(PropKey::from("s"), Type::Str)],
    ]
}

#[test]
fn an_inline_element_spans_one_word_per_field() {
    let l = layouts();
    assert_eq!(elem_stride(&l, Type::Inline(StructId(0))), 16);
    assert_eq!(elem_stride(&l, Type::Inline(StructId(1))), 24);
    for one_word in [Type::Obj(StructId(1)), Type::F64, Type::Any, Type::Str] {
        assert_eq!(elem_stride(&l, one_word), 8);
    }
}

#[test]
fn a_power_of_two_stride_shifts_and_any_other_multiplies() {
    let mut f = Function::new("f", Type::Void);
    let b = f.add_block();
    let i = Operand::ConstI64(5);
    for (stride, want) in [
        (8, InstKind::BinOp(BinOp::Shl, i, Operand::ConstI64(3))),
        (16, InstKind::BinOp(BinOp::Shl, i, Operand::ConstI64(4))),
        (24, InstKind::BinOp(BinOp::Mul, i, Operand::ConstI64(24))),
    ] {
        let v = emit_scaled_index(&mut f, b, i, stride);
        let inst = f.blocks[b.0 as usize].insts.last().unwrap();
        assert_eq!((inst.result, &inst.kind), (Some(v), &want));
        assert_eq!(f.value_type(v), Type::I64);
    }
}

#[test]
fn an_element_address_is_the_data_pointer_plus_the_offset() {
    let mut f = Function::new("f", Type::Void);
    let b = f.add_block();
    let data = f.append_inst(b, InstKind::Alloca(Type::Ptr), Type::Ptr, None);
    let off = Operand::ConstI64(32);
    let v = emit_elem_addr(&mut f, b, Operand::Value(data), off, StructId(0));
    let kinds: Vec<&InstKind> = f.blocks[b.0 as usize].insts[1..]
        .iter()
        .map(|i| &i.kind)
        .collect();
    let [
        InstKind::PtrToInt(d),
        InstKind::BinOp(BinOp::Add, Operand::Value(base), o),
        InstKind::IntToPtr(Operand::Value(sum)),
    ] = kinds.as_slice()
    else {
        panic!("{kinds:?}");
    };
    assert_eq!((*d, *o), (Operand::Value(data), off));
    assert_eq!(f.value_type(*base), Type::I64);
    assert_eq!(f.value_type(*sum), Type::I64);
    assert_eq!(f.value_type(v), Type::Inline(StructId(0)));
}

#[test]
fn a_field_sits_at_its_index_in_words_with_no_header() {
    let l = layouts();
    assert_eq!(field_slot(&l, StructId(0), "kind"), (0, Type::F64));
    assert_eq!(field_slot(&l, StructId(0), "value"), (8, Type::F64));
    assert_eq!(field_slot(&l, StructId(1), "c"), (16, Type::Bool));
}

#[test]
#[should_panic(expected = "has no `nope`")]
fn a_field_the_layout_lacks_is_an_internal_error() {
    field_slot(&layouts(), StructId(0), "nope");
}

#[test]
#[should_panic(expected = "inline field `s` is Str")]
fn a_refcounted_field_is_an_internal_error() {
    field_slot(&layouts(), StructId(2), "s");
}

#[test]
fn a_value_class_interns_in_place_in_every_spelling() {
    let mut t = ArrLayouts::with_value_classes(&[StructId(0)]);
    assert!(t.has_value_classes());
    let a = t.intern(Type::Obj(StructId(0)));
    assert_eq!(t[a.0 as usize], Type::Inline(StructId(0)));
    assert_eq!(t.intern(Type::Inline(StructId(0))), a);
    assert_eq!(t.intern(Type::Obj(StructId(0))), a);
    let other = t.intern(Type::Obj(StructId(1)));
    assert_eq!(
        (other, t[other.0 as usize]),
        (ArrId(1), Type::Obj(StructId(1)))
    );
    assert!(!ArrLayouts::default().has_value_classes());
    assert!(t.holds_in_place(StructId(0)) && !t.holds_in_place(StructId(1)));
}

#[test]
fn an_inline_array_releases_as_a_scalar_one() {
    for no_refs in [Type::I64, Type::F64, Type::Bool, Type::Inline(StructId(0))] {
        assert!(no_refs.elems_release_nothing(), "{no_refs:?}");
    }
    for refs in [
        Type::Obj(StructId(0)),
        Type::Str,
        Type::Any,
        Type::Arr(ArrId(0)),
    ] {
        assert!(!refs.elems_release_nothing(), "{refs:?}");
    }
}

#[test]
fn an_inline_array_carries_its_own_element_kind() {
    // the kind a struct field store or a stack literal writes into the
    // header; the cycle collector walks only the heap kind
    let mut t = ArrLayouts::with_value_classes(&[StructId(0)]);
    let inline = t.intern(Type::Obj(StructId(0)));
    let kind = u64::from(torajs_rc::ARR_KIND_INLINE);
    let chain = crate::ssa_lower_arr_kind_mark::arr_kind_chain_of;
    assert_eq!(chain(&t, &Type::Inline(StructId(0)), 0), kind);
    assert_eq!(chain(&t, &Type::Arr(inline), 0), 4 | (kind << 3));
    assert_ne!(kind, 4);
}
