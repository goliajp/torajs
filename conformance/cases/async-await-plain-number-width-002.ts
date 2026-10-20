// An await of an operand that may or may not be a promise
// (`number | Promise<number>`, `Promise<number> | null`) answers what
// the promise settled with, or the operand itself (ECMA-262 §27.7.5.3);
// a fraction keeps its fraction in an annotated binding, a pushed
// element and an inferred binding.

function half(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function pick(b: boolean): number | Promise<number> {
  if (b) return half(3);
  return 1;
}
function maybe(b: boolean): Promise<number> | null {
  if (b) return half(5);
  return null;
}

async function main() {
  const u = pick(true);
  const v: number = await u;
  console.log("union promise", v);
  const xs: number[] = [];
  xs.push(await u);
  xs.push(await pick(false));
  console.log("union push", xs[0], xs[1]);
  const w = await maybe(true);
  console.log("nullable promise", w);
}
main();
