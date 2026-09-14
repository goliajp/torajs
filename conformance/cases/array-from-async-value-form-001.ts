// RFC 20260914 — every spelling reaches the one self-hosted body: the direct call
// and `.call` / literal `.apply` by rewrite; an any receiver, a reified value and
// Reflect.apply through the runtime cell the program bound at startup. The cell
// still carries the reflection face (length 1, name).
async function main(): Promise<void> {
  const r1 = await (Array as any).fromAsync([1, 2]);
  console.log(r1.length, r1[0], r1[1]);
  const f: any = Array.fromAsync;
  const r2 = await f([3]);
  console.log(r2[0], Array.isArray(r2));
  console.log(Array.fromAsync.length, Array.fromAsync.name, typeof Array.fromAsync);
  const r3 = await f.call(undefined, "ab", (c: any) => c + "!");
  console.log(r3.join(","));
  const r4 = await Reflect.apply(Array.fromAsync, undefined, [[9]]);
  console.log(r4[0]);
  const r5 = await Array.fromAsync.apply(undefined, [[5, 6]]);
  console.log(r5.join(","));
  const r6 = await f({ length: 1, 0: Promise.resolve("p") });
  console.log(r6[0]);
  console.log("done");
}
main();
