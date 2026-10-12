// a nested async generator declaration reads the enclosing function's
// params, destructured params, let / const locals and arrow params,
// is callable above its own text, recurses, and keeps its name / length
async function collect(it: any): Promise<any[]> {
  const out: any[] = [];
  for await (const v of it) out.push(v);
  return out;
}

function outer(k: number) {
  function* gen() { yield k; yield k + 1 }
  async function* ag() { yield k }
  return [gen.name, ag.name, [...gen()].join(","), ag()] as const;
}

function mk(k: number, [a, b]: number[], { c }: { c: string }) {
  let l = 10;
  const m = "m";
  const early = ag2(1);
  async function* ag(x: number) { yield k; yield l; yield m; yield a + b; yield c; yield x }
  async function* ag2(y: number) { yield y + k }
  async function* rec(n: number): AsyncGenerator<number> {
    if (n > 0) { yield n; yield* rec(n - 1) }
  }
  {
    const before = inBlock();
    async function* inBlock() { yield k * 2 }
    l = 11;
    return [ag(5), early, rec(3), before, ag.name, ag.length, ag2.length, rec.name] as const;
  }
}

const viaArrow = (q: string) => {
  const it = ga();
  async function* ga() { yield q + "!" }
  return it;
};

async function inAsync(k: number) {
  const early = ag();
  async function* ag() { yield k; yield k + 1 }
  return await collect(early);
}

function plainNames() {
  async function* lone() { yield 1 }
  function* slone(p: number, q: number) { yield p + q }
  return [lone.name, slone.name, slone.length];
}

async function main() {
  const o = outer(3);
  console.log(o[0], o[1], o[2]);
  console.log(await collect(o[3]));
  const r = mk(1, [2, 3], { c: "cc" });
  console.log(await collect(r[0]));
  console.log(await collect(r[1]));
  console.log(await collect(r[2]));
  console.log(await collect(r[3]));
  console.log(r[4], r[5], r[6], r[7]);
  console.log(await collect(viaArrow("hi")));
  console.log(await inAsync(7));
  console.log(plainNames());
}
main();
