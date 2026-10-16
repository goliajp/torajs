use super::*;
use crate::nanbox::{VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_double, box_int32, box_void_ptr};

/// A plan whose root is `node` of `kind` over `tag`, optional when
/// `opt`, with one class list, `list`.
fn plan_of(opt: bool, kind: u8, tag: u32, list: &[u32]) -> Vec<u8> {
    let mut p = vec![0; 4];
    if opt {
        p.push(OPT);
    }
    p.push(kind);
    p.extend(tag.to_le_bytes());
    p.extend(0u32.to_le_bytes());
    let lists_off = p.len() as u32;
    p[..4].copy_from_slice(&lists_off.to_le_bytes());
    p.extend(1u32.to_le_bytes());
    p.extend((list.len() as u32).to_le_bytes());
    list.iter().for_each(|t| p.extend(t.to_le_bytes()));
    p
}

fn cast(plan: &[u8], v: AnyValue) -> Option<u64> {
    unsafe { as_is(&Plan::new(plan.as_ptr(), core::ptr::null()), 4, v) }
}

/// A live heap cell: refcount 1, heap tag `tag`, header `flags`, and
/// `word` where a `Tag::Obj` cell keeps its class tag.
fn cell(tag: Tag, flags: u16, word: u32) -> [u64; 4] {
    [
        1 | (tag as u64) << 32 | (flags as u64) << 48,
        word as u64,
        0,
        0,
    ]
}

fn boxed(c: &mut [u64; 4]) -> AnyValue {
    box_void_ptr(c.as_mut_ptr().cast())
}

fn refcount(c: &[u64; 4]) -> u64 {
    c[0] & 0xffff_ffff
}

#[test]
fn a_witness_node_takes_its_layout_and_the_classes_of_its_list() {
    let plan = plan_of(false, WIT, 5, &[42]);
    for class in [5, 42] {
        let mut c = cell(Tag::Obj, 0, class);
        let v = boxed(&mut c);
        assert_eq!(cast(&plan, v), Some(v));
        assert_eq!(refcount(&c), 2);
    }
}

#[test]
fn a_witness_node_refuses_another_class_and_other_cells() {
    let plan = plan_of(false, WIT, 5, &[42]);
    let mut other = cell(Tag::Obj, 0, 43);
    let mut map = cell(Tag::Map, 0, 42);
    let mut string = cell(Tag::Str, 0, 42);
    for c in [&mut other, &mut map, &mut string] {
        assert_eq!(cast(&plan, boxed(c)), None);
        assert_eq!(refcount(c), 1);
    }
    let empty = plan_of(false, WIT, 5, &[]);
    let mut listed = cell(Tag::Obj, 0, 42);
    assert_eq!(cast(&empty, boxed(&mut listed)), None);
}

#[test]
fn a_sub_node_takes_a_cell_of_its_tag_minted_for_a_listed_class() {
    let class = TEST_SUBCLASS_TAG as u32;
    let plan = plan_of(false, SUB, Tag::Map as u32, &[class]);
    let mut c = cell(Tag::Map, torajs_rc::FLAG_SUBCLASSED, 0);
    let v = boxed(&mut c);
    assert_eq!(cast(&plan, v), Some(v));
    assert_eq!(refcount(&c), 2);
    let elsewhere = plan_of(false, SUB, Tag::Map as u32, &[class + 1]);
    assert_eq!(cast(&elsewhere, v), None);
    assert_eq!(refcount(&c), 2);
}

#[test]
fn a_sub_node_refuses_a_plain_builtin_cell_and_other_tags() {
    let plan = plan_of(false, SUB, Tag::Map as u32, &[TEST_SUBCLASS_TAG as u32]);
    let mut plain = cell(Tag::Map, 0, 0);
    let mut set = cell(Tag::Set, torajs_rc::FLAG_SUBCLASSED, 0);
    let mut string = cell(Tag::Str, 0, 0);
    let mut obj = cell(Tag::Obj, torajs_rc::FLAG_SUBCLASSED, 0);
    for c in [&mut plain, &mut set, &mut string, &mut obj] {
        assert_eq!(cast(&plan, boxed(c)), None);
        assert_eq!(refcount(c), 1);
    }
}

#[test]
fn an_optional_node_takes_nullish_values_and_refuses_immediates() {
    for kind in [SUB, WIT] {
        let plan = plan_of(true, kind, Tag::Map as u32, &[TEST_SUBCLASS_TAG as u32]);
        assert_eq!(cast(&plan, VALUE_UNDEFINED), Some(undef_cell_ptr() as u64));
        assert_eq!(cast(&plan, VALUE_NULL), Some(0));
        for v in [box_int32(5), VALUE_TRUE, box_double(0.5)] {
            assert_eq!(cast(&plan, v), None);
        }
    }
    let strict = plan_of(false, SUB, Tag::Map as u32, &[TEST_SUBCLASS_TAG as u32]);
    assert_eq!(cast(&strict, VALUE_UNDEFINED), None);
    assert_eq!(cast(&strict, VALUE_NULL), None);
}
