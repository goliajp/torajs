// ES §14.11 — `for (var i = …; …)` inside a `with` body.
// Sloppy goal only, so this fixture is `.cts`.
//
// The same split as any other `var` in the body, except the two halves
// cannot stay where they are: the loop's init slot holds ONE statement.
// So the declaration is lifted in front of the loop and the slot keeps
// only the assignment — a shape the loop already accepts. Everything
// the loop then touches (the init write, the condition, the step) is
// an ordinary reference resolved through the object.
//
// What each block pins:
//   - the object carries the counter: the init write, the step and the
//     condition all go through it, so the loop drives `o.i` and the
//     hoisted binding stays undefined;
//   - the object does not carry it: the loop drives the hoisted
//     binding, exactly as without the `with`;
//   - the loop body's own references resolve the same way;
//   - a property added to the object DURING the loop starts winning at
//     the next reference, because the test is per reference;
//   - `for (var i; …)` with no initialiser declares and writes
//     nothing.

var o: any = { i: 100, seen: "object" };

with (o) {
  for (var i = 0; i < 3; i++) {
    // reads the object's counter, not the hoisted binding
    o.log = (o.log === undefined ? "" : o.log) + i;
  }
}
// The loop drove the object's property to its exit value; the hoisted
// binding never got written.
console.log(o.i, i, o.log);

// The object does not carry `j`, so the loop drives the hoisted one.
var plain: any = {};
with (plain) {
  for (var j = 0; j < 2; j++) {}
}
console.log(plain.j, j);

// No initialiser: the declaration writes nothing, so an object
// property of that name is untouched until the loop's own step.
var k: any = { n: 5 };
with (k) {
  for (var n; false; ) {}
}
console.log(k.n, n);

// The membership test is per reference: a property that appears
// mid-loop starts winning from the next mention.
var grow: any = {};
var acc = "";
with (grow) {
  for (var g = 0; g < 3; g++) {
    acc = acc + ":" + g;
    if (g === 0) {
      grow.g = 10;
    }
  }
}
console.log(acc, grow.g, g);
