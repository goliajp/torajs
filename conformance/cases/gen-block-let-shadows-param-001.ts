// Rotation 636 — a block-scoped `let` inside a generator body that
// reuses a param's name or the generator's own name is a distinct
// binding: the lift renames it instead of folding it onto the param's
// field, so reads outside the block still see the param / the function.
function* B4(x: number) {
  { let B4 = 0; B4 = 5; console.log("inner", B4); }
  { let x = 9; console.log("ix", x); }
  console.log("outer", typeof B4, x);
  yield x;
  for (let x = 100; x < 102; x++) { console.log("fx", x); }
  console.log("after", x);
}
const it = B4(3);
console.log(it.next().value);
console.log(it.next().done);
