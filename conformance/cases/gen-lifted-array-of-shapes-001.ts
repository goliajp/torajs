// A generator local holding an array of things the lift knows, and a
// `Promise.resolve` of one of them.
//
// The lift asks two inference halves what an initializer is: its own,
// which knows the shapes that only exist at this point in the pipeline
// (a `new C()`, a `Promise` static, a class's hoisted generator
// method), and the shared sniff, which knows everything else. They
// were composed for the initializer as a whole but not *inside* it, so
// a shape the lift knew stopped being known the moment it appeared as
// somebody's sub-expression: the shared sniff's array arm recursed
// into its own arms only, and `Promise` is a namespace it cannot type.
//
// `const ps = [Promise.resolve(2.5)]` therefore took the `number`
// fallback and the checker rejected the store outright — "field is
// Number, value is Array(Promise(Number))" — as did `[new C()]` and
// `Promise.resolve(new C())`.

class Box {
  v: number = 4;
}

// an array of promises: uniform, so the element type is the anchor's
function* uniformPromises(): any {
  const ps = [Promise.resolve(2.5)];
  yield ps.length;
  yield ps[0];
}
const up = uniformPromises();
console.log(up.next().value);
up.next().value.then(
  (v: any) => {
    console.log("resolved", v);
    return 0;
  },
  (e: any) => {
    console.log("bad", e);
    return 0;
  },
);

// a mixed literal widens to any[] — the anchor's type would stamp an
// annotation the Arr<Any> lowering cannot satisfy
function* mixed(): any {
  const xs = [Promise.resolve(1), 2, Promise.resolve("s")];
  yield xs.length;
  yield xs[1];
}
const mx = mixed();
console.log(mx.next().value);
console.log(mx.next().value);

// an array of class instances
function* boxes(): any {
  const cs = [new Box(), new Box()];
  cs[1].v = 7;
  yield cs[0].v + cs[1].v;
  yield cs.length;
}
const bs = boxes();
console.log(bs.next().value);
console.log(bs.next().value);

// `Promise.resolve` of a class instance — the argument's own shape is
// what the promise's value is (§27.2.4.7)
function* resolvedBox(): any {
  const p = Promise.resolve(new Box());
  yield p;
}
resolvedBox().next().value.then(
  (b: any) => {
    console.log("box", b.v);
    return 0;
  },
  (e: any) => {
    console.log("bad", e);
    return 0;
  },
);

// an all-integer literal still reads as number[] — the shared sniff
// answered this one before and answers it the same way now
function* narrow(): any {
  const ns = [1, 2, 3];
  yield ns[0] + ns[2];
}
console.log(narrow().next().value);
