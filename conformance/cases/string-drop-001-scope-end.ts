// P3.1-g.6 — exercise __torajs_str_drop via heavy str-local scope churn.
// All Str-typed locals drop at scope end; this exercises the dec + free path.
function build_and_drop(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const a: string = "key_" + i;
    const b: string = a + "_suffix";
    total += b.length;
  }
  return total;
}
console.log(build_and_drop(50));
// Mixed types to ensure cross-type drop dispatch still works.
const arr: string[] = ["a", "b", "c"];
for (const s of arr) {
  console.log(s + "!");
}
console.log("ok");
