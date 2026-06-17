// `String(arr)` per ES §22.1.3.30 routes through arr_join(",") —
// the same dispatch as `arr.toString()`. Element-type picks the
// matching arr_join intrinsic (i64 / f64 / bool / substr / generic).
console.log(String([1, 2, 3]))
console.log(String([1.5, 2.5, 3.5]))
console.log(String([true, false, true]))
console.log(String(['a', 'b', 'c']))
console.log(String([42]))
console.log(String([]))

// `String(struct)` per ES §20.1.4.4 generic Object toString.
console.log(String({a: 1}))
console.log(String({name: 'alice', age: 30}))
console.log(String({x: 1, y: 2, z: 3}))

// Mixed with template / concat — String(arr) callsite.
const xs = [10, 20, 30]
console.log('xs as string: ' + String(xs))
