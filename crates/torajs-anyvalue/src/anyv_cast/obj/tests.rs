use super::super::plan::{ANY, BOOL, BOXED, INT, NUM, OPT, STR};
use super::*;
use crate::nanbox::{VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_double, box_int32};
use torajs_rc::undef_cell::undef_cell_ptr;

/// A plan whose root is `node`, with an empty lists table.
fn plan_of(node: &[u8]) -> Vec<u8> {
    let lists_off = (4 + node.len()) as u32;
    let mut p = lists_off.to_le_bytes().to_vec();
    p.extend_from_slice(node);
    p.extend_from_slice(&0u32.to_le_bytes());
    p
}

fn cast(node: &[u8], v: AnyValue) -> Option<u64> {
    let bytes = plan_of(node);
    let plan = Plan::<false>::new(bytes.as_ptr(), core::ptr::null());
    unsafe { plan.cast(4, v) }.ok()
}

#[test]
fn scalar_nodes_take_values_of_their_type() {
    assert_eq!(cast(&[INT], box_double(4.0)), Some(4));
    assert_eq!(cast(&[INT], box_double(4.5)), None);
    assert_eq!(cast(&[NUM], box_int32(2)), Some(2.0f64.to_bits()));
    assert_eq!(cast(&[BOOL], VALUE_TRUE), Some(1));
    assert_eq!(cast(&[BOOL], box_int32(1)), None);
    assert_eq!(cast(&[STR], VALUE_TRUE), None);
}

#[test]
fn an_any_node_takes_any_primitive_as_it_is() {
    for v in [VALUE_UNDEFINED, VALUE_NULL, VALUE_TRUE, box_int32(9)] {
        assert_eq!(cast(&[ANY], v), Some(v));
    }
}

#[test]
fn a_boxed_member_takes_its_kind_and_nullish_values() {
    assert_eq!(cast(&[BOXED, 0], box_double(2.5)), Some(box_double(2.5)));
    assert_eq!(cast(&[BOXED, 0], VALUE_NULL), Some(VALUE_NULL));
    assert_eq!(cast(&[BOXED, 0], VALUE_TRUE), None);
    assert_eq!(cast(&[BOXED, 1], VALUE_TRUE), Some(VALUE_TRUE));
}

#[test]
fn an_optional_number_takes_undefined_as_the_sentinel_and_refuses_null() {
    assert_eq!(
        cast(&[OPT, NUM], VALUE_UNDEFINED),
        Some(torajs_rc::F64_UNDEF_SENTINEL_BITS)
    );
    assert_eq!(cast(&[OPT, NUM], VALUE_NULL), None);
    assert_eq!(cast(&[OPT, NUM], box_int32(3)), Some(3.0f64.to_bits()));
    assert_eq!(cast(&[OPT, NUM], VALUE_TRUE), None);
}

#[test]
fn an_optional_object_takes_nullish_values_as_their_spellings() {
    let obj = [
        OPT, OBJ, 0, 1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(cast(&obj, VALUE_UNDEFINED), Some(undef_cell_ptr() as u64));
    assert_eq!(cast(&obj, VALUE_NULL), Some(0));
    assert_eq!(cast(&obj, box_int32(1)), None);
}
