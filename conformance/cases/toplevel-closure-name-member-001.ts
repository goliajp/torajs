// An array or object literal whose MEMBER names a closure binding,
// read from a function declaration. A named fn body has no capture
// machinery, so it reads top-level bindings through the globals
// table — and one such member kept the whole binding out of it,
// because the any-slot verdict only admitted a closure written
// INLINE at the member position. So
//
//     const arr = [k];   function give() { return arr[0](3) }
//
// answered "unknown identifier arr", while `[(a: number) => …]`
// spelled at the same position worked, and so did `const arr: any[]
// = [k]`.
//
// A member that names such a binding holds the identical value the
// inline spelling does, reached through one hop, so it boxes the
// same way. The single-declaration / non-`var` argument that makes
// the hop safe is the alias rule's, stated once and shared.
let tag = (a: number) => "x" + a;
let half = (a: number) => a / 4;

const arr = [tag];
function viaArr() {
  return arr[0](3);
}
// Both homes: the main path and the named-fn path read one binding.
console.log(arr[0](1), viaArr());

const bag = { f: half, n: 1 };
function viaBag() {
  return bag.f(9) + bag.n;
}
// A fractional result rules out an integer-width slot reading f64
// bits back as a garbage integer.
console.log(bag.f(9), viaBag());

// Nested, and mixed with a member that names a different binding.
const grid = [[tag]];
const both = { a: half, b: tag };
function viaNested() {
  return grid[0][0](3) + both.b(4);
}
console.log(grid[0][0](1), viaNested(), both.a(9));

// A member naming a PRIMITIVE binding still admits the way it
// always did — this arm only widens.
const size = 5;
const sizes = [size];
const holder = { s: size };
function viaPrims() {
  return sizes[0] + holder.s;
}
console.log(viaPrims(), sizes[0] + 1, holder.s + 1);
