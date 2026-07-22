// P0 chained-Array<Any>-temp bug regression fixture (2026-07-23):
// `.filter()` on a temporary heap-element array (Array<Any> from
// `.map()`, or a string[] literal) lost element ownership because
// emit_filter pushed the borrowed src slot directly into dst without
// rc-incing the payload. When src dropped (chain temp), dst's slot
// dangled → later `.join()` / stringify read freed memory.
// Fix: emit_filter now calls emit_owned_result_inc(elem, elem_ty)
// inside the push branch, no-op on Copy scalars, rc_inc on typed
// heap kinds, any_box_rc_inc on Type::Any.
const anyArr: any[] = [1, 2, 3]
console.log(anyArr.map((n) => n.toString()).filter((s) => s.length > 0).join('-'))
console.log(
  JSON.stringify(anyArr.map((n) => n.toString()).filter((s) => s.length > 0)),
)

const strArr: string[] = ['a', 'b', 'c']
console.log(strArr.map((s) => s + '!').filter((s) => s.length > 0).join('-'))

const numArr: number[] = [1, 2, 3, 4, 5]
console.log(numArr.map((n) => n * 2).filter((n) => n > 0).join('-'))
