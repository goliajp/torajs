// T-10.d.i (v0.4.0) — `xs[i]` indexed read on Array<Any> + console.log
// Any-dispatch. The indexed read calls __torajs_any_box(tag, value)
// to lift the array slot into a single SSA-carryable Operand;
// console.log of an Any operand routes through __torajs_print_any
// which dispatches by tag.

let xs: any[] = [42, 'hello', true]
console.log(xs[0])
console.log(xs[1])
console.log(xs[2])

// Mixed integer + string + boolean — exercises the three primary
// primitive tags (ANY_I64 / ANY_HEAP→TAG_STR / ANY_BOOL).
let mixed: any[] = [1, 'a', false, 99, 'b', true]
console.log(mixed[3])
console.log(mixed[4])
console.log(mixed[5])

// Larger array to push past the cap=4 initial alloc and trigger the
// 2x-grow path of __torajs_arr_push_any.
let many: any[] = [10, 'x', true, 20, 'y', false, 30, 'z']
console.log(many.length)
console.log(many[7])
