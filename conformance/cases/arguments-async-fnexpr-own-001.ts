// A function expression returned from an ASYNC function lost its own
// `arguments` when the outer body read `arguments` too: the async
// rewrite spells `return f` as `return Promise.resolve(f as any)`, and
// the argv-face kill walk knew every legal use of a fn-value binding
// but an argument of a Promise settle static — not an FnDecl, so the
// explicit-`any`-param admit never saw it — and killed the face:
// `unknown identifier arguments`, then a rejection carrying garbage.
// The settled value is read back only through the any lane (the
// await unwrap, a `.then` handler, an any-lane call), which enters
// the closure cell's boxed dual entry with real argc/argv — the same
// claim the escape-store profile makes for a store INTO
// `Promise.resolve` — so the argument admits like any other any slot.
async function viaLocal() {
  let a = arguments;
  const f = function () {
    return a === arguments;
  };
  return f;
}
viaLocal(1).then((f: any) => f()).then((r: any) => console.log("local", r));
async function viaReturn(x: number) {
  let a = arguments;
  return function () {
    return a === arguments;
  };
}
viaReturn(1).then((f: any) => f(2)).then((r: any) => console.log("return", r));
async function viaAsyncInner(x: number) {
  let a = arguments;
  return async function () {
    return a === arguments;
  };
}
viaAsyncInner(1).then((f: any) => f()).then((r: any) => console.log("async-inner", r));
function syncOuter() {
  let a = arguments;
  const g = function () {
    return a === arguments;
  };
  return Promise.resolve(g as any);
}
syncOuter(1).then((f: any) => f()).then((r: any) => console.log("sync-outer", r));
async function ownLength() {
  return function () {
    return arguments.length;
  };
}
ownLength().then((f: any) => f(1, 2, 3)).then((n: any) => console.log("own-length", n));
