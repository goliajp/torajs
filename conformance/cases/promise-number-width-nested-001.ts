// A promise handed in through `any` keeps the fractions nested in what
// it settles with (ECMA-262 §27.7.5.3): the elements of a
// `Promise<number[]>`, the field of a `Promise<{ n: number }>`, and a
// class field typed `Promise<number>`, each read once from an integer
// writer and once from a fraction the program's types cannot see.

class Holder {
  p: Promise<number>;
  constructor(p: Promise<number>) {
    this.p = p;
  }
}

function objOf(text: string): any {
  const n: number = JSON.parse(text);
  return Promise.resolve({ n: n });
}

async function main() {
  const ai: any = Promise.resolve([1, 2]);
  const pi: Promise<number[]> = ai;
  console.log("elements int", (await pi)[0] + (await pi)[1]);
  const af: any = Promise.resolve(JSON.parse("[1.5, 2]"));
  const pf: Promise<number[]> = af;
  const xs = await pf;
  console.log("elements frac", xs[0] + xs[1], (await pf)[0]);

  const oi: any = Promise.resolve({ n: 2 });
  const qi: Promise<{ n: number }> = oi;
  console.log("field int", (await qi).n + 1);
  const qf: Promise<{ n: number }> = objOf("2.5");
  const o = await qf;
  console.log("field frac", o.n + 1);

  const hc: any = Promise.resolve(JSON.parse("3.5"));
  const c = new Holder(hc);
  console.log("class field frac", (await c.p) + 1);
  await c.p.then((v: number) => console.log("class field then", v * 2));
  const hd: any = Promise.resolve(JSON.parse("5.5"));
  c.p = hd;
  console.log("class field store", (await c.p) * 2);
  const ci = new Holder(Promise.resolve(4));
  console.log("class field int", (await ci.p) + 1);
}
main();
