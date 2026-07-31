// §14.3.2.1 — a `var` inside a nested statement list (try / block /
// switch case) binds at FN/script scope even when its init shape
// (array / object / function literal) previously kept it as a
// block-scoped let (the rotation 264 escape-hatch fix): reads and
// writes after the block see the same binding.
try {
  var a = [1, 2];
  a.length = 1;
} catch (e) {}
try {
  a = [];
} catch (e) {}
console.log("try", a.length);

{
  var o = { k: 3 };
}
console.log("block", o.k);

switch (1) {
  case 1:
    var f = function (): number {
      return 7;
    };
    break;
}
console.log("switch", f());

// The dominant test262 shape: a var declared in one block, mutated
// from another, read at top level.
function counted(): number {
  var GET_COUNT = 0;
  if (true) {
    var bump = function (): void {
      GET_COUNT += 1;
    };
  }
  bump();
  bump();
  return GET_COUNT;
}
console.log("fn", counted());

// Typed-annotation var in a nested block also hoists (the annotation
// is a hint, not a slot constraint).
if (true) {
  var n: number = 5;
}
n = n + 1;
console.log("typed", n);
