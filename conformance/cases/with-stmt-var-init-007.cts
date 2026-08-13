// ES §14.11 + §14.3.2 — a `var` declaration inside a `with` body.
// Sloppy goal only, so this fixture is `.cts`.
//
// The classic: the DECLARATION hoists to the enclosing function's
// variable environment, which sits BEHIND the object record, while
// the INITIALISER is an assignment evaluated where it is written, IN
// FRONT of it. So when the object carries the name, `var v = 2`
// writes `o.v` and leaves the hoisted binding undefined — the two
// halves of one statement land on opposite sides of the object.
//
// What each block pins:
//   - object carries the name: the write goes to the object and the
//     hoisted binding keeps whatever it had (undefined if nothing);
//   - object does not carry it: the write lands on the hoisted
//     binding, exactly as without the `with`;
//   - `var v;` with no initialiser declares and writes nothing, so an
//     object property of that name is untouched;
//   - the binding really is function-scoped: it is readable after the
//     block, and readable before it (hoisted, undefined);
//   - a `var` nested in a block inside the body splits the same way;
//   - several names in one declaration each split;
//   - inside a NESTED function the same `var` is an ordinary local
//     and the object never sees it.

var o: any = { v: 1, kept: "object" };

// Hoisted, so readable before the statement that declares it.
console.log(v);

with (o) {
  var v = 2;
  var fresh = 3;
}

// The write went to the object; the hoisted binding never got it.
console.log(o.v, v);
// The object does not carry `fresh`, so that one landed on the
// hoisted binding.
console.log(o.fresh, fresh);

// No initialiser: nothing is written, so the object's property stands.
with (o) {
  var kept;
}
console.log(o.kept, kept);

// A `var` already carrying a value elsewhere keeps it when the write
// is captured by the object.
var held = "held";
var h: any = { held: "object-held" };
with (h) {
  var held = "reassigned";
}
console.log(h.held, held);

// Nested in a block inside the body.
var n: any = { deep: "object-deep" };
with (n) {
  if (true) {
    var deep = "written";
  }
}
console.log(n.deep, deep);

// Several names in one declaration, one carried and one not.
var m: any = { a: "object-a" };
with (m) {
  var a = "written-a",
    b = "written-b";
}
console.log(m.a, a, m.b, b);

// Inside a nested function the same `var` is an ordinary local: it is
// bound in front of the object, so the object never sees the write.
var f: any = { local: "object-local" };
var run: any = null;
with (f) {
  run = function (): string {
    var local = "own";
    return local;
  };
}
console.log(run(), f.local);
