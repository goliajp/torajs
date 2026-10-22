// Promise.all over typed inputs whose result holds heap values or
// whose element form the call site does not name.
//
// A string or object result co-owns each element: the source promises
// die right after the call and the result must still read its values.
// An input reached through `any` names no form, and its result must
// still read every element as itself.

type Pt = { x: number; y: number };

function mk(i: number): Promise<string>[] {
  return [Promise.resolve("s" + i), Promise.resolve("t" + i)];
}

async function main() {
  const kept: string[] = [];
  for (let i = 0; i < 3; i++) {
    const r = await Promise.all(mk(i));
    kept.push(r[0] + r[1]);
  }
  console.log(kept.join(","));

  const pts = await Promise.all([Promise.resolve({ x: 1, y: 2 }), Promise.resolve({ x: 3, y: 4 })]);
  let sum = 0;
  for (const p of pts) {
    const q: Pt = p;
    sum += q.x * q.y;
  }
  console.log("pts", sum);

  const anyIn: any = [Promise.resolve(1), Promise.resolve(2.5), Promise.resolve("z")];
  const u = await Promise.all(anyIn);
  console.log("any", u[0], u[1], u[2]);

  Promise.all([Promise.resolve(1), Promise.resolve(2.5)]).then((vs) => {
    console.log("widths", vs[0] + vs[1]);
  });
}

main();
