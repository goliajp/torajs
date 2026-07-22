// rotation 191 — `Array<T>.map(cb)` where cb returns U ≠ T.
// Before the fix `infer_anonymous_closure_params` strapped the cb's
// return_type = elem_ann (`"map" => (params, elem_ann)`) and the
// method-table arm declared map's shape as `((T) => T) => T[]`, so
// `numbers.map(n => n.toString())` tripped `check_stmt_return` on
// the arrow body's String return (before the call-site type ever
// answered) — the P0 handoff report ("mv/mv2.ts").
//
// Fix routes cb param through the elem seed but leaves the return
// annotation for the body sniff, and adds a call-site wedge
// (`check_type_of_call_arr_map_hetero`) that admits primitive U
// (Number / String / Boolean / Any) and answers `Array<U>`.
// Homogeneous `(T) => T` keeps the method-table arm; Void-ret keeps
// the sister `arr_pred_void_cb` wedge (boxes `undefined` into
// `Any[]`).
//
// ssa_lower's `emit_map` already reads `dst_arr_ty` off the call
// result and pushes via `raw_slot_arg` (F64 bit-cast + heap
// pass-through), so no lowering change was needed for these lanes.

// T=number → U=string
const xs: number[] = [1, 2, 3]
console.log('to_str:', xs.map(n => n.toString()).join('-'))

// T=number → U=number (homogeneous — method-table arm)
console.log('sqr:', xs.map(n => n * n).join('-'))

// T=number → U=boolean
console.log('even:', xs.map(n => n % 2 === 0).join('-'))

// chained: map to string + filter + join (the heap-elem-rc bug
// probe target from handoff, now byte-parity)
console.log('chain:', xs.map(n => n.toString()).filter(s => s.length > 0).join('/'))

// string[] → number[]
const words: string[] = ['a', 'bb', 'ccc']
console.log('lens:', words.map(w => w.length).join('-'))

// string[] → boolean[]
console.log('long:', words.map(w => w.length > 1).join('-'))
