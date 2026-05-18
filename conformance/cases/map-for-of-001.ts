// P6.4c-C2 — for-of dispatch for Map / Set / MapIter via the P6.4b
// MapIter substrate. Map's default iter (spec §23.1.4) is .entries()
// yielding `[k, v]` Array<Any> pairs; this fixture exercises:
//   1. Direct iter binding usage: `for (let pair of m.entries())`
//   2. Destructuring on Map default iter: `for (let [k, v] of m)`
//   3. Keys / values explicit: `for (let k of m.keys())` / `.values()`
//   4. Set default iter: `for (let v of s)` (yields elements per
//      spec §24.2.5.1).

let m: Map = new Map()
m.set('a', 1)
m.set('b', 2)
m.set('c', 3)

// (1) For-of with Map default iter — bind pair as Array<Any>.
// var_ty is Arr<Any> per lower_for_of_map_like for Map source so
// pair[0] / pair[1] go through the typed Array<Any> Index path.
console.log('--- entries pair ---')
for (let pair of m) {
  console.log(typeof pair)
  console.log(pair[0])
  console.log(pair[1])
}

// (2) Destructuring on Map default iter — parser-side desugar
// pre-pends `let k = __forof_destr[0]; let v = __forof_destr[1]`.
console.log('--- destructure ---')
for (let [k, v] of m) {
  console.log(k)
  console.log(v)
}

// (3) keys / values explicit — receiver is Type::MapIter, var is
// Type::Any (kind unknown statically; runtime delivers either key
// or value).
console.log('--- keys ---')
for (let k of m.keys()) {
  console.log(typeof k)
}
console.log('--- values ---')
for (let v of m.values()) {
  console.log(typeof v)
}

// (4) Set default iter — yields elements, var is Type::Any.
let s: Set = new Set()
s.add('alpha')
s.add('beta')
console.log('--- set ---')
for (let elem of s) {
  console.log(typeof elem)
}
