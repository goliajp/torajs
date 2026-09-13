// An async function VALUE whose body declares an async function that
// names itself: the declaration becomes `let d = async function () {
// … d … }` before the body moves, and `d` is the body's own binding —
// not a capture of the enclosing scope for the hoist to snapshot
// (`unknown identifier d`). Same shape as test262
// `AsyncDisposableStack/prototype/disposeAsync/disposes-resources-in-
// reverse-order` (`stack.defer(dispose5)` from inside `asyncTest`).
function run(fn: any) { return fn(); }
run(async function () {
  var seen: any[] = [];
  async function d() { seen.push(d); return d; }
  const same = (await d()) === d;
  const stack = new AsyncDisposableStack();
  async function dispose5() { seen.push(dispose5); }
  stack.defer(dispose5);
  await stack.disposeAsync();
  console.log("self-ref", same, seen.length, seen[0] === d, seen[1] === dispose5);
});
