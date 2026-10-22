// A closed program whose only computed keys are loop counters and
// arithmetic of them: typed array reads and writes under `i`, `i + 1`
// and `n - 1 - i`, promises of numbers resolved and awaited.

const n = 6;
const xs: number[] = [];
for (let i = 0; i < n; i = i + 1) {
  xs[i] = i * i;
}
const ps: Promise<number>[] = [];
for (let i = 0; i < n; i++) {
  ps.push(Promise.resolve(xs[n - 1 - i]));
}

async function main() {
  const r: number[] = await Promise.all(ps);
  let t = 0;
  for (let i = 0; i + 1 < r.length; i += 1) {
    t = t + r[i] - r[i + 1];
  }
  const last = await Promise.resolve(r[r.length - 1]);
  console.log(r.join(","), t, last);
}

main();
