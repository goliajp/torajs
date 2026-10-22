// A `new Promise<T>` executor whose `resolve` is called from a nested
// closure, handed a promise of `any`, or passed on settles with a value
// of any kind; the promise is read as it is.

function nested(x: any): Promise<number> {
  return new Promise<number>((r) => {
    const go = () => r(x);
    go();
  });
}

function adopted(x: any): Promise<number> {
  return new Promise<number>((r) => r(Promise.resolve(x)));
}

function call(k: (v: any) => void, x: any) {
  k(x);
}

function passed(x: any): Promise<number> {
  return new Promise<number>((r) => call(r, x));
}

function aliased(x: any): Promise<number> {
  return new Promise<number>((r) => {
    const k = r;
    k(x);
  });
}

function exec(r: (v: any) => void) {
  r("named");
}

function named(): Promise<number> {
  return new Promise<number>(exec);
}

function typed(): Promise<number> {
  return new Promise<number>((r) => r(41));
}

async function main() {
  const a = await nested("s");
  console.log(typeof a, a);
  const b = await adopted("t");
  console.log(typeof b, b);
  const c = await passed("u");
  console.log(typeof c, c);
  const d = await aliased("v");
  console.log(typeof d, d);
  const e = await named();
  console.log(typeof e, e);
  const f = await typed();
  console.log(typeof f, f + 1);
  nested("p").then((v) => console.log("then", typeof v, v));
}

main();
