// §9.1.1.4.6 SetMutableBinding with `S` = false — an assignment to an
// unresolvable name creates the property on the GLOBAL OBJECT, at run
// time. The distinction matters where the write is conditional: after
// `desugar_with` splits a body reference into "object has it" and
// "object does not", the two `with` blocks below read the same
// statically and only the object decides which arm runs.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS.
var o1: any = { foo: 1 };
with (o1) {
  foo = 42;
}
try {
  foo;
  console.log("no throw");
} catch (e) {
  console.log((e as any).name);
}
console.log(o1.foo, "foo" in globalThis);

var o2: any = {};
with (o2) {
  bar9 = 5;
}
console.log(bar9, globalThis.bar9, "bar9" in globalThis);

baz9 = 7;
console.log(typeof baz9, globalThis.baz9);
console.log(typeof neverWritten9);
