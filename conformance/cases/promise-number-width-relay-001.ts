// A promise from `any` keeps its fraction however typed code relays it
// before the read (ECMA-262 §27.7.5.3): through a typed parameter and
// a local of an async function, a closure's result, an object field a
// function builds, `.then` inside an async function, an alias of the
// result, and an async function awaiting another that read an `any`.

function frac(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
const box: any[] = [frac(5), frac(9), frac(11), frac(13), frac(15)];

async function relay(p: Promise<number>): Promise<number> {
  const q = p;
  return await q;
}
async function relay2(f: () => Promise<number>): Promise<number> {
  const q = f();
  const r = q;
  return await r;
}
function wrap(p: Promise<number>): { p: Promise<number> } {
  return { p };
}
async function viaThen(): Promise<number> {
  const h: any = box[2];
  const p: Promise<number> = h;
  return p.then((v) => v);
}
async function inner(): Promise<number> {
  const h: any = box[4];
  const v: number = await h;
  return v;
}
async function outer(): Promise<number> {
  return await inner();
}

async function main() {
  const h: any = box[0];
  const hp: Promise<number> = h;
  console.log("relay", await relay(hp));
  console.log("relay int", await relay(frac(4)));
  console.log("relay2", await relay2(() => box[1]));
  const h3: any = box[3];
  const w = wrap(h3 as Promise<number>);
  console.log("wrap", await w.p);
  console.log("viaThen", await viaThen());
  const t = viaThen();
  const u = t;
  console.log("alias", await u);
  console.log("outer", await outer());
  const o = outer();
  console.log("outer then", await o.then((x) => x + 1));
}
main();
