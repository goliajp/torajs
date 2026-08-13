// §14.11 + §14.3.2 — a destructuring `var` inside a `with` body.
//
// The declaration belongs to the enclosing function, BEHIND the object
// record, so the name does not shadow the object and the write lands
// on the property. A `let` pattern is the opposite on both counts, and
// is here as the control: it really does bind in front of the record,
// so the object is left alone.
//
// This only became visible once the pattern desugar carried the
// `var`-ness of its declaration — while every pattern name came out
// block-scoped, both halves below looked like the `let` one.
//
// `.cts` because `with` only exists under the sloppy goal.

var src: any = { a: 1, b: 2 };
var arr: any = [10, 20];

var o: any = { a: "obj-a", b: "obj-b", x: "obj-x" };

with (o) {
  var { a, b } = src;
  var [x] = arr;
}
// The object took every write.
console.log(o.a, o.b, o.x);
// The hoisted bindings exist and never got one.
console.log(typeof a, typeof b, typeof x);

var p: any = { c: "kept" };
with (p) {
  let { c } = src;
  // Bound in front of the record, so this reads the pattern's binding.
  console.log("let sees", typeof c === "undefined" ? "undefined" : c);
}
// And the object was never written.
console.log(p.c);
