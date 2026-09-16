// §20.2.3.2 steps 5-7 and 8-14 read the target's `length` and `name`
// when `.bind` runs, not when the bound function is asked. Resolving
// them lazily made every bound function a live view of its target:
// a `defineProperty` after the bind moved the answer.

function two(a: number, b: number): number {
  return a + b;
}
const B = two.bind(null);
console.log(B.length, B.name);
Object.defineProperty(two, "length", { value: 7 });
Object.defineProperty(two, "name", { value: "renamed" });
// the OLD bound function keeps what it read at mint...
console.log(B.length, B.name);
// ...and a fresh one takes the new values.
console.log(two.bind(null).length, two.bind(null).name);

// same on the any lane, partials subtracted from what was read then
const anyB: any = (two as any).bind(null, 1);
Object.defineProperty(two, "length", { value: 42 });
console.log(anyB.length, anyB.name, String(anyB));

// bound-over-bound resolves each layer at its own mint
function three(a: number, b: number, c: number): number {
  return a + b + c;
}
const t1 = three.bind(null, 1);
const t2 = t1.bind(null, 2);
console.log(t1.length, t1.name);
console.log(t2.length, t2.name);
Object.defineProperty(three, "length", { value: 0 });
console.log(t2.length, t2.name);

// a builtin method target reads its own arity, once
const up: any = ("x" as any).toUpperCase.bind("x");
console.log(up.length, up.name, up());
