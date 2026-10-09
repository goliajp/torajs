// a binding moved out of a globalThis.X read's way keeps its own name in
// the dead-zone error, and a parameter default does not see the body's
// declarations of the same name
function tdz(n: number): number {
  try {
    console.log(Math);
  } catch (e) {
    console.log((e as Error).message);
  }
  try {
    console.log(typeof Math);
  } catch (e) {
    console.log((e as Error).name, (e as Error).message);
  }
  let Math = n;
  return globalThis.Math.abs(Math);
}
console.log(tdz(-3));

function dflt(a: any = typeof Math, n = 1): [any, number, number] {
  const Math = n;
  return [a, globalThis.Math.abs(-2), Math];
}
console.log(dflt());
console.log(dflt("given", 7));
