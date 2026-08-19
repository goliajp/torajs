// §27.2.4.{1,3,5,6} — every combinator's step 1/2 reads |this| as
// the species constructor. The reified cells are recv-first
// (this_aware_id): a `.call` / `.bind` spelling with the builtin
// Promise as receiver runs the same iterator-interleaved dyn kernel
// the direct spelling does; a bare detached call (undefined |this|)
// and a non-Promise receiver keep the step-1 TypeError bun/JSC
// raises. Custom species constructors (NewPromiseCapability(C))
// stay the recorded follow-up.
async function main() {
  const a = (Promise as any).all;
  const r1 = await a.call(Promise, [1, 2, 3]);
  console.log("all.call", r1[0], r1[1], r1[2]);
  const r2 = await (Promise as any).race.call(Promise, [Promise.resolve(9)]);
  console.log("race.call", r2);
  const r3 = await (Promise as any).any.call(Promise, [Promise.reject(1), 5]);
  console.log("any.call", r3);
  const r4: any = await (Promise as any).allSettled.call(Promise, [1]);
  console.log("allSettled.call", r4.length);
  const bound = (Promise as any).all.bind(Promise);
  const r5 = await bound([7]);
  console.log("all.bind", r5[0]);
  try {
    a([1]);
  } catch (e: any) {
    console.log("detached-bare", e instanceof TypeError);
  }
  try {
    a.call({}, [1]);
  } catch (e: any) {
    console.log("wrong-this", e instanceof TypeError);
  }
}
main();
