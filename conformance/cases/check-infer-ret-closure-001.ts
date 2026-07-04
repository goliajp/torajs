// chunk 522 — return-type inference for fns returning closure-typed
// locals: the lifted-closure signatures pre-infer ahead of user fns,
// and a captured top-level closure binding upgrades the ret slot to
// the Closure ABI.
function mk() {
  const g = (x: number) => x * 2;
  return g;
}
console.log(mk()(21));
// arrow returning a captured top-level closure
const g = (x: number) => x + 5;
const h = () => g;
console.log(h()(10));
// the returned value is re-callable through a binding
const f = mk();
console.log(f(4));
// closure factory with a captured base
function adder(base: number) {
  const add = (x: number) => x + base;
  return add;
}
console.log(adder(100)(23));
console.log("done");
