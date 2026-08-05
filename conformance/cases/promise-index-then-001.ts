// An element of a `Promise<T>[]` is as much a built-in promise as a
// field of that type is, so `ps[0].then(cb)` chains like `c.p.then(cb)`.
//
// The chain lowering asks `expr_types` whether the receiver is a
// promise for a field read and for an optional chain, but an INDEX
// read was not in that arm — so `ps[0].then(cb)` reached the
// resolve_callee panic ("not yet supported: ssa-lower: unsupported
// member call shape: then") while the identical program written with
// the element bound to a name first lowered fine. Both spellings
// appear below.

const ps: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2)];

// the spelling that did not lower
ps[0].then((v: number) => {
  console.log("indexed", v);
  return 0;
});

// the same thing through a binding — always worked
const p1 = ps[1];
p1.then((v: number) => {
  console.log("bound", v);
  return 0;
});

// a computed index
const i = 0;
ps[i].then((v: number) => {
  console.log("computed", v);
  return 0;
});

// catch on an indexed receiver
const qs: Promise<number>[] = [Promise.reject(7)];
qs[0].catch((e: any) => {
  console.log("caught", e);
  return 0;
});

// an indexed receiver inside a struct field, one level further out
class Holder {
  items: Promise<number>[] = [];
}
const h = new Holder();
h.items = [Promise.resolve(9)];
h.items[0].then((v: number) => {
  console.log("field-indexed", v);
  return 0;
});
