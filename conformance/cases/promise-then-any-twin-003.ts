// Lanes the any twin must leave alone or take over, beyond the plain
// number / string / boolean ones: a `string | undefined` handler handed
// undefined, a pointer-lane handler handed null and an object (whose
// fields it does not read), and integer-valued handlers fed from typed
// integer promises and from an `any` holding an integer. §27.2.5.4
// calls each handler with the value as it is.

type Pt = { x: number };

const u: any = undefined;
const n: any = null;
const pt: any = { x: 5 };
const seven: any = 7;
const half: any = 2.5;

async function main() {
  const p1: Promise<string | undefined> = Promise.resolve(u);
  await p1.then((v: string | undefined) => {
    console.log("str|undef", typeof v, v === undefined);
  });
  const p1b: Promise<string | undefined> = Promise.resolve("s");
  await p1b.then((v: string | undefined) => {
    console.log("str|undef fits", typeof v, v);
  });

  const p2: Promise<Pt | null> = Promise.resolve(n);
  await p2.then((v: Pt | null) => {
    console.log("pt|null", v === null, typeof v);
  });
  const p2b: Promise<Pt | null> = Promise.resolve(pt);
  await p2b.then((v: Pt | null) => {
    console.log("pt|null fits", v === null, typeof v);
  });

  let sum = 0;
  for (let i = 0; i < 4; i++) {
    sum = await Promise.resolve(sum).then((v: number) => v + i * 3);
  }
  console.log("int chain", sum);

  const p3: Promise<number> = Promise.resolve(seven);
  console.log("int from any", await p3.then((v: number) => v * 6));
  const p4: Promise<number> = Promise.resolve(half);
  console.log("frac from any", await p4.then((v: number) => v * 2));
  const p5: Promise<number> = Promise.resolve(n);
  await p5.then((v: number) => {
    console.log("null->number", typeof v, v === null);
  });
}

main();
