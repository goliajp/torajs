// §7.1.19 ToPropertyKey(undefined) is the fixed string key
// "undefined", and an array takes it like any other property key —
// it is not a canonical array index, so it never touches the element
// lane and never moves `length`.
//
// The checker has admitted this pair since rotation 346. The read
// lowering did not: its lane list spelled `Any | String | Symbol`
// while the gate admitted `Undefined` too, so this program
// type-checked and then died ("cannot coerce Ptr to i64"). And the
// two halves spelled the same key differently — `undefined` and
// `null` both lower to a null pointer, so the read's box called it
// ANY_NULL while the write's called it ANY_UNDEF.
const x = [1, 2]
console.log(x[undefined])
x[undefined] = 9
console.log(x["undefined"], x[undefined], x.length, x[0], x[1])

// the element lane is untouched: a canonical index spelling still
// reads and writes elements, and length still tracks them
x["0"] = 7
console.log(x[0], x["0"], x.length)

// and the rest of the array's key domain still answers where it did
const sk = Symbol("s")
x[sk] = 5
let kd = "1"
console.log(x[sk], x[kd], x["length"], x["nope"], x.length)
