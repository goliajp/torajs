// A `Promise<number>` field first given a promise of an integer and
// then, through `any`, one of a fraction reads the fraction back
// (ECMA-262 §27.7.5.3): in a class field, an object literal's field,
// a Map value and a local class's field.

function frac(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}

function ints(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}

class Box {
  p: Promise<number> = ints(1);
}

const h: any = frac(13);

async function main() {
  const b = new Box();
  console.log(await b.p);
  b.p = h;
  console.log(await b.p);
  const o: { p: Promise<number> } = { p: ints(2) };
  o.p = h;
  console.log(await o.p);
  const m = new Map<string, Promise<number>>();
  m.set("k", h);
  const g = m.get("k");
  if (g) console.log(await g);
  class Local {
    p: Promise<number> = ints(3);
  }
  const l = new Local();
  l.p = frac(7);
  console.log(await l.p);
}

main();
