// Rest-parameter contextual typing inside generator and async bodies.
function* gen(): Generator<number> {
  const f: (...xs: number[]) => number = (...xs) => xs.length;
  yield f(1, 2, 3);
  const fs: Array<(...xs: number[]) => number> = [];
  fs.push((...xs) => xs.reduce((a, b) => a + b, 0));
  yield fs[0](1, 2, 3);
}
for (const x of gen()) console.log(x);

async function run(): Promise<void> {
  const f: (a: string, ...xs: number[]) => string = (a, ...xs) => a + xs.length;
  console.log(f("n", 4, 5));
  await 0;
  const g: (...xs: string[]) => string = (...xs) => xs.join("+");
  console.log(g("a", "b"));
}
run();
