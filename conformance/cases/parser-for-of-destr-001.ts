// V3-18 wedge — for-of loop with array-destructuring pattern:
//   for (let [a, b] of pairs) { ... }
// Common shape when iterating arrays-of-tuples or paired
// streams. Pre-fix tora's parser bailed at the pattern site
// with 'expected `=` after destructuring pattern, got
// Ident("of")'.
//
// Subset limitation: object-destructuring (`for (let {x, y} of
// objs)`) not yet supported — would need member rather than
// index access on the synthesized per-iteration temp.

for (let [a, b] of [[1, 2], [3, 4], [5, 6]]) {
  console.log(a, b)
}

let pairs: number[][] = [[10, 20], [30, 40], [50, 60]]
for (let [x, y] of pairs) {
  console.log(x + y)
}

// const variant.
for (const [k, v] of [[100, 200], [300, 400]]) {
  console.log(k * v)
}

// Pattern with one binding — degenerates to index [0].
for (let [first] of [[1, 99], [2, 99], [3, 99]]) {
  console.log(first)
}
