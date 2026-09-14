// proposal-array-from-async §2.1.1 steps 3.h.i / 3.i.iv — `Array.fromAsync.call(C, …)`
// constructs through C (no argument for an iterable source, the length for an
// array-like one), defines each element, then Set(A, "length", k, true): a
// read-only length rejects with a TypeError. A non-constructor |this| takes
// ArrayCreate. The `as any` spelling takes the runtime cell route to the same body.
class MyArr {
  n: number;
  constructor(n: any) {
    this.n = n === undefined ? -1 : n;
  }
}
function ReadOnlyLen(this: any) {
  Object.defineProperty(this, "length", {
    enumerable: true,
    writable: false,
    configurable: true,
    value: 99,
  });
}
async function main(): Promise<void> {
  const r1: any = await Array.fromAsync.call(MyArr, [1, 2]);
  console.log(r1 instanceof MyArr, r1.n, r1[0], r1[1], r1.length);
  const r2: any = await Array.fromAsync.call(MyArr, { length: 2, 0: "a", 1: "b" });
  console.log(r2 instanceof MyArr, r2.n, r2[0], r2[1], r2.length);
  const r3: any = await (Array.fromAsync as any).call(MyArr, [7]);
  console.log(r3 instanceof MyArr, r3.n, r3[0], r3.length);
  try {
    await Array.fromAsync.call(ReadOnlyLen, [0, 1, 2]);
    console.log("BAD: iterable fulfilled");
  } catch (e) {
    console.log("ro-iter", e instanceof TypeError);
  }
  try {
    await Array.fromAsync.call(ReadOnlyLen, { length: 1, 0: 0 });
    console.log("BAD: array-like fulfilled");
  } catch (e) {
    console.log("ro-like", e instanceof TypeError);
  }
  try {
    await Array.fromAsync.call({}, { length: 4294967296 });
    console.log("BAD: too-long fulfilled");
  } catch (e) {
    console.log("too-long", e instanceof RangeError);
  }
  const r4: any = await Array.fromAsync.call(undefined, [3]);
  console.log(Array.isArray(r4), r4[0]);
  console.log("done");
}
main();
