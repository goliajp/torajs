// Untyped rest param is implicitly `any[]` (TS implicit-any): the
// parser synthesizes the annotation, so the mandatory-annotation
// checker face and every lowering path see the user-written-`any[]`
// shape. Also covers the unmapped-arguments interaction: a rest
// param makes the arguments object UNMAPPED (ES §10.4.4.7), so
// `arguments[0] = 2` must not write through to `a`.
let value = 0;
function rest(a: number, ...b) {
  arguments[0] = 2;
  value = a;
}
rest(1);
console.log(value);

function collect(a: number, ...b) {
  console.log(a, b.length);
}
collect(1, "x", true);

class C {
  m(...items) {
    console.log(items.length);
  }
}
new C().m(1, 2, 3);
