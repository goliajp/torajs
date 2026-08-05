// A generator local holding what `Promise.resolve` answers.
//
// The shared sniff types a method call from its receiver's
// annotation, and `Promise` is a namespace rather than a value it can
// type — so the whole call declined and the local took the `number`
// fallback: "field is Number, value is Promise(Array(Number))".
//
// §27.2.4.7 makes the value rule precise, and both halves of it are
// exercised below: handing `resolve` a plain value makes that value
// the promise's, and handing it a promise passes that promise's value
// through rather than nesting.
//
// `reject` and the combinators still decline. Each has its own value
// rule — `allSettled` answers an array of outcome objects, not of
// values — and a wrong answer here would pin the field just as badly
// as the fallback it replaces.

function* g(): any {
  const arr = Promise.resolve([1, 2, 3]);
  yield arr;

  const num = Promise.resolve(7);
  yield num;

  const str = Promise.resolve("hi");
  yield str;

  // pass-through: resolving a promise does not nest it
  const inner = Promise.resolve(41);
  const outer = Promise.resolve(inner);
  yield outer;

  // no argument at all — fulfils with undefined
  const empty = Promise.resolve();
  yield empty;
}

const it = g();
it.next().value.then(function (xs: any): void {
  console.log("arr", xs[0], xs[2]);
});
it.next().value.then(function (n: any): void {
  console.log("num", n);
});
it.next().value.then(function (s: any): void {
  console.log("str", s);
});
it.next().value.then(function (n: any): void {
  console.log("outer", n);
});
it.next().value.then(function (v: any): void {
  console.log("empty", v);
});
