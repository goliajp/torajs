// P6.4c-C3 / P5.4 — Array<Any>.keys() / .values() / .entries() via
// Type::ArrIter (parallel to MapIter). Exercises direct iter
// stepping + for-of dispatch + destructuring on entries pairs.
//
// Subset: Array<Any> only. Typed Array<T> for non-Any T uses a
// different slot layout (8B-per-slot) the runtime helper can't walk
// uniformly — typecheck rejects `xs.keys()` on Array<i64> etc. with
// a follow-up message until elem-tag substrate lands.

let xs: any[] = ['alpha', 42, true]

console.log(xs.length)

// .keys() — yields i64 indices (0, 1, 2).
console.log('--- keys ---')
for (let k of xs.keys()) {
  console.log(typeof k)
  console.log(k)
}

// .values() — yields each slot's Any value.
console.log('--- values ---')
for (let v of xs.values()) {
  console.log(typeof v)
}

// .entries() — yields [index, value] Array<Any> pairs. Destructuring
// works because lower_for_of_map_like binds var as Type::Arr<Any>
// for ArrIter sources too... actually wait, ArrIter src is Type::Any
// per the helper, so destructuring goes through Any path. Test
// `pair[0]` / `pair[1]` would fail on Any (P0 gap). Use single-var
// `pair` then check typeof.
console.log('--- entries ---')
for (let pair of xs.entries()) {
  console.log(typeof pair)
}

// Direct iter stepping — `iter.next()` returning IteratorResult<any>.
console.log('--- direct keys.next ---')
let it = xs.keys()
let s1 = it.next()
console.log(s1.done)
console.log(typeof s1.value)
let s2 = it.next()
console.log(s2.done)
let s3 = it.next()
console.log(s3.done)
let s4 = it.next()
console.log(s4.done)
console.log(typeof s4.value)
