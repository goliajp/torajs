// `xs.reduce(cb)` with an `any` callback was refused at lowering —
// "call_fn_value: expected Closure or FnSig, got Any" — while
// `xs.reduce(cb, 0)` ran. One method, two arities, two answers.
//
// The 1-arg overload has its own early checker arm, and it claimed
// the call without looking at the callback. Answering the ELEMENT
// type there is also a decision about lowering: a typed call runs the
// inline loop, whose per-element call needs the callback's static
// signature. The 2-arg spelling was never claimed by that arm, fell
// through to the any-callback route, and ran on the runtime kernel —
// which is why only one of the two worked.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const add: any = (a: number, b: number) => a + b;
const padd: any = new Proxy((a: number, b: number) => a + b, {});
const xs = [1, 2, 3];

console.log("1-arg any    ", attempt(() => xs.reduce(add)));
console.log("1-arg proxy  ", attempt(() => xs.reduce(padd)));
console.log("1-arg right  ", attempt(() => xs.reduceRight(add)));
console.log("2-arg any    ", attempt(() => xs.reduce(add, 10)));
console.log("2-arg right  ", attempt(() => xs.reduceRight(add, 10)));

// The accumulator seeds from arr[0] / arr[len-1], so the order is
// observable and the two directions must not agree by accident.
const cat: any = (a: any, b: any) => String(a) + String(b);
console.log("order l-to-r ", attempt(() => ["a", "b", "c"].reduce(cat)));
console.log("order r-to-l ", attempt(() => ["a", "b", "c"].reduceRight(cat)));

// §23.1.3.24 step 3 — an empty array with no initial value throws.
console.log("empty no init", attempt(() => ([] as number[]).reduce(add)));

// Not callable stays a TypeError.
const bad: any = {};
console.log("not callable ", attempt(() => xs.reduce(bad)));

// The typed lane, one branch unchanged: a statically-typed callback
// keeps the inline loop and both arities still answer.
console.log("typed 1-arg  ", attempt(() => xs.reduce((a: number, b: number) => a + b)));
console.log("typed 2-arg  ", attempt(() => xs.reduce((a: number, b: number) => a + b, 10)));
