// Rotation 543 — `xs.keys()` was excluded from the element-kind mark
// on a true observation: it yields indices and never reads a slot, so
// the STEP needs no mark. But the step is not the only consumer.
//
// In `const t = xs.keys()` the iterator holds the last strong ref to
// the source, so the array dies inside `__torajs_arr_iter_drop` ->
// `__torajs_value_drop_heap`, and THAT reads the recorded elem kind
// to decide whether the slots hold cells. Unmarked, it freed the
// block and left every element behind.
//
// 200k churn, AOT product RSS, 1.51 MB flat baseline:
//   const zs = ["a" + i];            zs.keys()   8.00 MB -> 1.74 MB
//   const zs = ["a" + i, "b" + i];   zs.keys()  14.47 MB -> 1.77 MB
//
// The control that named it: the same loop over `[1, 2]` measured
// 1.61 MB throughout — a numeric array's slots have nothing to drop,
// so the missing mark costs nothing. `.values()` and `.entries()`,
// which mark, were flat at 1.79 / 1.77 MB before and after.
const a = ["x", "y"];
console.log([...a.keys()], [...a.values()], [...a.entries()]);

const n = [1, 2, 3];
console.log([...n.keys()], [...n.values()]);

const p = ["p", "q"];
for (const i of p.keys()) {
  console.log(i, p[i]);
}

console.log(["z"].keys().next().value, ["z"][0]);

const dyn = ["a" + 1, "b" + 2];
console.log([...dyn.keys()], dyn[0], dyn[1], dyn.length);
