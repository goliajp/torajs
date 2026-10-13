// The same contextual typing inside generator and async bodies. An
// async body with no await is wrapped in a try, and the annotations
// of the bindings inside that try were not read at all, so even the
// let-annotated arrow below kept untyped parameters there.
function* gen(): Generator<string> {
  const k: number[] = [0];
  const ws: Array<(v: number) => void> = [];
  ws.push((v) => { k[0] = v; });
  ws[0](42);
  yield String(k[0]);
  const out: string[] = [];
  const ss: Array<(s: string, n: number) => void> = [];
  ss.push((s, n) => { out.push(s + n); });
  ss[0]("a", 1);
  yield out[0];
  const fs: ((x: number) => number)[] = [];
  fs.push((x) => x * 2);
  yield String(fs[0](21));
}
for (const x of gen()) console.log(x);

async function noAwait(): Promise<void> {
  const k: number[] = [0];
  const ws: Array<(v: number) => void> = [];
  ws.push((v) => { k[0] = v; });
  ws[0](7);
  console.log(k[0]);
  const lit: ((v: number) => void)[] = [(v) => { k[0] = v + 1; }];
  lit[0](7);
  console.log(k[0]);
  const f: (v: number) => void = (v) => { k[0] = v + 2; };
  f(7);
  console.log(k[0]);
  try {
    const g: (s: string) => number = (s) => s.length;
    console.log(g("four"));
  } finally {
    console.log("done");
  }
}

async function withAwait(): Promise<void> {
  const k: number[] = [0];
  const ws: Array<(v: number) => void> = [];
  ws.push((v) => { k[0] = v; });
  await 0;
  ws[0](9);
  console.log(k[0]);
}

async function main(): Promise<void> {
  await noAwait();
  await withAwait();
}
main();
