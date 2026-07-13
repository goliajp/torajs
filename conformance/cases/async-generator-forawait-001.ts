// RFC 20260713-generator-fn-value-substrate blade 4 step 2 —
// `for await (... of ag(...))` over an async generator declaration:
// each next() step is awaited (Promise<step> per §27.6), the loop
// binding narrows to the yield type, and an early break exits.

async function* ag(n: number): AsyncGenerator<number> {
  yield n;
  yield n + 1;
  yield n + 2;
}

(async () => {
  for await (const v of ag(10)) {
    console.log("fa", v);
  }
  console.log("first done");
  for await (const v of ag(100)) {
    console.log("fb", v);
    if (v > 100) {
      break;
    }
  }
  console.log("second done");
})();
