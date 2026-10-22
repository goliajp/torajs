// An async function whose return reads a binding initialized from an
// `any` parameter settles with that value as it is, whatever the
// binding's annotation or the function's declared return says.

async function annotated(x: any): Promise<number> {
  const n: number = x;
  return n;
}

async function awaited(x: any): Promise<number> {
  const n: number = await Promise.resolve(x);
  return n;
}

async function shadowed(x: any): Promise<number> {
  let n = 3;
  {
    let n = x;
    return n;
  }
}

async function bare(x: any) {
  const n: number = x;
  return n;
}

async function bareShadowed(x: any) {
  let n = 3;
  {
    let n = x;
    return n;
  }
}

async function diff(a: any, b: any) {
  return a - b;
}

async function typed(): Promise<number> {
  const n: number = 40;
  return n + 2;
}

async function main() {
  const a = await annotated("s");
  console.log(typeof a, a);
  const b = await awaited("t");
  console.log(typeof b, b);
  const c = await shadowed("u");
  console.log(typeof c, c);
  const d = await bare("v");
  console.log(typeof d, d);
  const e = await bareShadowed("w");
  console.log(typeof e, e);
  const f = await diff(7n, 2n);
  console.log(typeof f, f);
  const g = await typed();
  console.log(typeof g, g + 1);
  annotated("p").then((v) => console.log("then", typeof v, v));
  bare("q").then((v) => console.log("then", typeof v, v));
}

main();
