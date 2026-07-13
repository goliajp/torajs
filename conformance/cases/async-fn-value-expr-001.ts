// RFC 20260713-generator-fn-value-substrate blade 3 — async
// function-VALUE expressions: async arrow (IIFE / bound / shorthand),
// async function expression, capture through the closure env,
// throw → rejected Promise, await inside the body, and the
// `async function*` expression (hoist + blade 4 decl machinery).

// IIFE async arrow — the canonical silent-wrong shape pre-blade.
const r = (async () => {
  console.log("async arrow ran");
  return 1;
})();
r.then((v: any) => console.log("then", v));

// Bound async function expression, called then chained.
const f = async function () {
  console.log("async fnexpr ran");
  return 2;
};
f().then((v: any) => console.log("then", v));

// Bound async arrow with a parameter.
const g = async (x: number) => x * 10;
g(4).then((v: any) => console.log("then", v));

// Capture: async arrow closing over an enclosing-function local.
function main() {
  const base = 100;
  const h = async (x: number) => base + x;
  h(5).then((v: any) => console.log("cap", v));
}
main();

// Uncaught throw inside an async body becomes a rejected Promise.
const err = async () => {
  throw "boom";
};
err().catch((e: any) => console.log("caught", e));

// await inside the async body.
const chain = async () => {
  const a = await Promise.resolve(7);
  return a * 2;
};
chain().then((v: any) => console.log("awaited", v));

// `async function*` expression — hoisted, factory returns the
// generator object, each next() resolves to a step.
const ag = async function* (n: number): AsyncGenerator<number> {
  yield n * 100;
  yield n * 100 + 1;
};
(async () => {
  const it = ag(7);
  let s = await it.next();
  while (!s.done) {
    console.log("agexpr", s.value);
    s = await it.next();
  }
  console.log("wrapper done");
})();
