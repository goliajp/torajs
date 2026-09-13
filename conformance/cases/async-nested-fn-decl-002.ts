// A `function` declared directly in an async body becomes a function
// value bound in its slot before the body moves into its state machine
// (`desugar_async_nested_fns`): a nested declaration could not see the
// machine's fields ("unknown identifier"), and an async function VALUE
// declaring one was refused the move and kept the synchronous lowering,
// where an await of an already-settled promise continued in the same
// job — one tick early (`async-disposable-stack-dispose-sync-calls`).
const t: string[] = [];
Promise.resolve().then(() => t.push("t1")).then(() => t.push("t2")).then(() => t.push("t3")).then(() => console.log(t.join(" ")));
function helper(fn: any) { fn(); }
helper(async function () {
  async function inner() { Promise.resolve().then(() => t.push("before")); return 5; }
  await inner();
  t.push("after");
});
async function outer() { let n = 1; function g() { return n; } await null; n = 2; console.log("decl", g()); }
outer();
helper(async function () {
  let values: any[] = [];
  async function inner() {
    const d = { [Symbol.asyncDispose]() { values.push(42); } };
    const e = { m() { values.push(43); } };
    e.m();
    await (d as any)[Symbol.asyncDispose]();
  }
  await inner();
  console.log("captured", JSON.stringify(values));
});
async function rec() { function fact(n: number): number { return n <= 1 ? 1 : n * fact(n - 1); } await null; console.log("rec", fact(5)); }
rec();
