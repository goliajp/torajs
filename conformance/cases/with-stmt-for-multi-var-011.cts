// §14.11 + §14.3.2 — several declarators in one `for (var …)` head
// inside a `with` body.
//
// A `var` is two things in two scopes: the DECLARATION belongs to the
// enclosing function, BEHIND the object record, while the INITIALISER
// is a write evaluated in front of it. So when the object carries the
// name, the loop writes the property and the hoisted binding is left
// undefined — which is what the first block checks.
//
// The head can hold only one statement, so the declarations lift out
// in front of the loop and the writes stay in the slot joined by the
// comma operator: `for (i = 0, j = 1; …)`, which is what the source
// means anyway.
//
// `.cts` because `with` only exists under the sloppy goal.

var o: any = { i: "obj", j: "obj" };

with (o) {
  for (var i = 0, j = 1; i < 2; i++) {
    j = j + 1;
  }
}
// The object took every write; the hoisted bindings never got one.
console.log(o.i, o.j);
console.log(typeof i, typeof j);

// A declarator with no initialiser writes nothing, so only its
// neighbours reach the object.
var p: any = { m: "obj" };
with (p) {
  for (var m = 0, n; m < 2; m++) {
    n = m;
  }
  console.log("inside", m, n);
}
console.log(p.m, typeof p.n);
