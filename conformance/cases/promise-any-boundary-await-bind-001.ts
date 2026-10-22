// A binding annotated with a primitive over an `await` that reads a
// value of any kind holds what was settled: the annotation has no
// runtime meaning, so `typeof` and the arithmetic see the real value.

async function anyOf(x: any): Promise<any> {
  return x;
}

async function main() {
  const x: any = JSON.parse('"str"');
  const n: number = await Promise.resolve(x);
  console.log("resolve", typeof n, n);
  const m: number = await anyOf(x);
  console.log("call", typeof m, m + 1);
  const s: string = await anyOf(JSON.parse("5"));
  console.log("string", typeof s, s);
  const k: number = await anyOf(JSON.parse("41"));
  console.log("number", typeof k, k + 1);
  const t: number = await Promise.resolve(2);
  console.log("typed", typeof t, t * 3);
}
main();
