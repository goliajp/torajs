// V3-18 wedge — for-of with object-destructuring pattern:
//   for (let { x, y } of pts) { ... }
// per ES spec §13.7.5. Pre-fix tora's parse_for_of only
// recognized array destructuring (`for (let [a, b] of pairs)`)
// since the obj form needs Member access (`<iter>.x`) instead
// of Index access (`<iter>[0]`). The obj form is the dominant
// shape for iterating arrays of typed records — this wedge
// lets `for ({ name, id } of users)` Just Work without the
// hoist-into-temp workaround.
//
// Implementation: in parse_for_of (parser.rs/try_parse_for_of)
// add a sibling branch to the existing array-destr branch:
//   * destruct_obj: Option<Vec<(field, bound)>>
//   * iterator var hoisted to `__forof_destr_<id>` (same as
//     array destr)
//   * body pre-prepended with `let bound = <iter>.field` for
//     each entry, wrapped in a fresh block.
// Reserved-word fields go through keyword_property_name
// (matching the wedge that landed for parse_object_destructuring).

let pts: { x: number, y: number }[] = [{x:1,y:2}, {x:3,y:4}, {x:5,y:6}]
for (let { x, y } of pts) {
  console.log(x, y)
}
// 1 2
// 3 4
// 5 6

// Rename target.
for (let { x: px, y: py } of pts) {
  console.log("p:", px, py)
}
// p: 1 2
// p: 3 4
// p: 5 6

// Extra fields on the source are ignored.
let users = [{ id: 1, name: "alice" }, { id: 2, name: "bob" }]
for (let { name } of users) {
  console.log(name)
}
// alice
// bob

// Multi-field with rename.
for (let { id: u, name: n } of users) {
  console.log(u, n)
}
// 1 alice
// 2 bob

// `const` form too.
let pairs: { a: number, b: number }[] = [{a:10,b:20}, {a:30,b:40}]
for (const { a, b } of pairs) {
  console.log(a + b)
}
// 30
// 70
