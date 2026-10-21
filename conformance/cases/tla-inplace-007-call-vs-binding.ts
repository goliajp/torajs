// Two shapes of a top-level await of a pending promise a job settles,
// many times over. (a) The operand is a call's result that no binding
// holds. (b) Two bindings hold the same promise: the first await waits
// for it, the second reads it already settled, one job later. Both
// read the settled value, and the promises are released as the loop
// goes on.
function pending(v: number): Promise<number> {
  return new Promise<number>((r) => {
    queueMicrotask(() => r(v));
  });
}

let sum = 0;
for (let i = 0; i < 100000; i = i + 1) {
  sum = sum + (await pending(i));
}
console.log("a", sum);

let total = 0;
for (let i = 0; i < 100000; i = i + 1) {
  const p = pending(i * 2);
  const q = p;
  const v = await p;
  total = total + v + (await q);
}
console.log("b", total);
