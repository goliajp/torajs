// An await in a body that suspends over a frame releases the promise
// it awaited, so an operand that reads a promise a binding still holds
// takes a reference of its own, under any spelling of the read: a
// comma, an assignment, a conditional, `??`, `?.`. Each binding is
// awaited again after every such read, three rounds.

function half(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}

async function main() {
  let p = half(3);
  let q = half(5);
  const o: { p: Promise<number> } | undefined = { p: half(7) };
  const ps: Promise<number>[] = [half(9)];
  const c = q !== p;
  for (let i = 0; i < 3; i++) {
    const a = await (0, p);
    const b = await (q = p);
    const d = await (c ? p : q);
    const e = await (p ?? q);
    const f = await o?.p;
    const g = await ps?.[0];
    console.log(i, a, b, d, e, f, g, await p, await q, await o.p, await ps[0]);
  }
}
main();
