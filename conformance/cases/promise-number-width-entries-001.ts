// A promise of a fraction handed in through `any` keeps its fraction
// at every place a `Promise<number>` can receive it (ECMA-262
// §27.2.5.4, §27.7.5.3): a parameter, a constructor parameter and a
// parameter property, a default parameter, an array element, a
// destructured binding, a Map value, an optional chain, a captured
// binding, a getter (class and object literal), a method returning
// `any`, a `.then` handler returning `any` (also down a chain), and an
// `any[]` given to a `Promise<number>[]`. Each section has helpers of
// its own and reads an integer through the same value point first.

function frac1(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints1(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h1: any = frac1(13);

async function param1(p: Promise<number>) {
  return await p;
}
async function s1() {
  console.log("param", await param1(ints1(1)), await param1(h1));
}

function frac2(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints2(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h2: any = frac2(13);

class Holder2 {
  p: Promise<number>;
  constructor(p: Promise<number>) {
    this.p = p;
  }
}
async function s2() {
  console.log("ctor", await new Holder2(ints2(1)).p, await new Holder2(h2).p);
}

function frac3(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints3(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h3: any = frac3(13);

class Prop3 {
  constructor(public p: Promise<number>) {}
}
async function s3() {
  console.log("ctor prop", await new Prop3(ints3(1)).p, await new Prop3(h3).p);
}

function frac4(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints4(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h4: any = frac4(13);

async function dflt4(p: Promise<number> = ints4(4)) {
  return await p;
}
async function s4() {
  console.log("default", await dflt4(), await dflt4(h4));
}

function frac5(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints5(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h5: any = frac5(13);
async function s5() {
  const xs: Promise<number>[] = [ints5(1)];
  console.log("elem", await xs[0]);
  xs[0] = h5;
  console.log("elem any", await xs[0]);
}

function frac6(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints6(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h6: any = frac6(13);
async function s6() {
  const { p: a }: { p: Promise<number> } = { p: ints6(1) };
  const { p: b }: { p: Promise<number> } = { p: h6 };
  const [c]: Promise<number>[] = [h6];
  console.log("destructure", await a, await b, await c);
}

function frac7(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints7(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h7: any = frac7(13);
async function s7() {
  const m = new Map<string, Promise<number>>();
  m.set("i", ints7(1));
  m.set("f", h7);
  for (const [k, v] of m) console.log("map", k, await v);
}

function frac8(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints8(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h8: any = frac8(13);
async function s8() {
  const src: any = { p: h8 };
  const o: { p: Promise<number> } | undefined = src;
  const oi: { p: Promise<number> } | undefined = { p: ints8(1) };
  const op = o?.p;
  const oip = oi?.p;
  if (op && oip) console.log("optional chain", await oip, await op);
}

function frac9(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints9(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h9: any = frac9(13);
async function s9() {
  const cap: Promise<number> = h9;
  const read = async () => await cap;
  console.log("capture", await ints9(1), await read());
}

function frac10(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints10(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h10: any = frac10(13);

class G10 {
  q: any = h10;
  get p(): Promise<number> {
    return this.q;
  }
}
async function s10() {
  console.log("getter", await ints10(1), await new G10().p);
  const g = new G10();
  const gp = g.p;
  console.log("getter local", await gp);
}

function frac11(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints11(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h11: any = frac11(13);
async function s11() {
  const lit = {
    get p(): Promise<number> {
      return h11;
    },
  };
  console.log("getter literal", await ints11(1), await lit.p);
}

function frac12(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints12(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h12: any = frac12(13);

class M12 {
  q: any = h12;
  get(): Promise<number> {
    return this.q;
  }
}
async function s12() {
  console.log("method", await ints12(1), await new M12().get());
}

function frac13(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints13(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h13: any = frac13(13);
async function s13() {
  const t: Promise<number> = ints13(1).then((v) => (v > 100 ? v : h13));
  console.log("then any", await ints13(2), await t);
}

function frac14(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints14(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h14: any = frac14(13);
async function s14() {
  const k: any = 2.5;
  const t2 = ints14(1)
    .then((v) => (v > 100 ? v : k))
    .then((v: number) => v + 1);
  console.log("then chain", await t2);
}

function frac15(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints15(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h15: any = frac15(13);
async function s15() {
  const arr: any[] = [h15];
  const ys: Promise<number>[] = arr.map((x) => x);
  console.log("any array", await ints15(1), await ys[0]);
}

async function main() {
  await s1();
  await s2();
  await s3();
  await s4();
  await s5();
  await s6();
  await s7();
  await s8();
  await s9();
  await s10();
  await s11();
  await s12();
  await s13();
  await s14();
  await s15();
}

main();
