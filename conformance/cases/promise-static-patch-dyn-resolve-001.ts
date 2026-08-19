// The dyn combinator lane's never-activated exit with a WORKING
// patched Promise.resolve: each element rides one patched call
// (§27.2.4.1.3 step 6.i, this = Promise), and the collected answers
// delegate straight to the any-lane kernel — the patch must be
// applied exactly once per element.
let calls = "";
(Promise as any).resolve = function (v: any) {
  calls += v;
  return new Promise<any>((res) => {
    res(v * 10);
  });
};

function* gen() {
  yield 1;
  yield 2;
  yield 3;
}

async function main() {
  const vals: any = await Promise.all(gen() as any);
  console.log("all", vals[0], vals[1], vals[2], "calls", calls);
  calls = "";
  const first: any = await Promise.race(gen() as any);
  console.log("race", first, "calls", calls);
}
main();
