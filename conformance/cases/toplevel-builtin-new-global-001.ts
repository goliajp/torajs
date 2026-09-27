// An un-annotated top-level binding initialized by `new` of a built-in
// is read from a function declaration's body. Each one used to stay
// local to the program body, so the function saw no binding at all:
// `typeof p` answered "undefined" and any other read threw a
// ReferenceError, while the same program with the type written out
// worked.
const target = { a: 1 };
const p = new Proxy(target, {
  get(t: any, k: any): any {
    return k === "a" ? t.a + 41 : undefined;
  },
});
const buf = new ArrayBuffer(8);
const view = new DataView(buf);
const ref = new WeakRef(target);
const registry = new FinalizationRegistry((_held: any) => {});

function report(): void {
  console.log(typeof p, p.a);
  view.setInt16(0, 513);
  console.log(buf.byteLength, view.getInt8(0), view.getInt8(1));
  console.log(ref.deref() === target);
  console.log(typeof registry, typeof registry.register);
}

report();
