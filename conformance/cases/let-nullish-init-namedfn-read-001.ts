// A nullish-initialised mutable `let` widens to `any` (rotation 651);
// a named-fn body must see it through the same boxed global slot the
// `: any` spelling gets — the test262 "was return() called with the
// right arguments" idiom writes it from an object-literal method and
// reads it from an async function.
let args = null;
let it: any = {
  m() {
    args = arguments;
    return { done: true };
  },
};
function readSync() {
  console.log("sync", args.length, args[1]);
}
async function readAsync() {
  await 0;
  console.log("async", args.length, args[0]);
  for await (const x of [1]) {
    console.log("for-await", args.length + x);
  }
}
it.m(7, 8);
readSync();
readAsync().then(() => {
  // `undefined` init, rebound from a plain function declaration
  let u = undefined;
  function w() { u = [1, 2, 3]; }
  function r() { console.log("undef-init", u.length); }
  w(); r();
  // top-level read stays as it was
  console.log("top", args.length);
});
