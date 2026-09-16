// Every `.bind` rides the runtime kernel — the static synth lane that
// used to swallow a typed `f.bind(...)` minted a second bound-function
// implementation with no [[Construct]] and with `length` / `name` read
// off the declaration instead of off the target.

// §10.4.1.2 — the bound product constructs its target.
function Point(this: any, x: number, y: number) {
  (this as any).x = x;
  (this as any).y = y;
}
const B = Point.bind(null, 1);
const p: any = new (B as any)(2);
console.log(p.x, p.y);
console.log(p instanceof (Point as any));

// §20.2.3.2 steps 5-7 / 12-14 — length and name come from reading the
// TARGET's own properties, which `defineProperty` can move.
function two(a: number, b: number): number {
  return a + b;
}
console.log(two.bind(null).length, two.bind(null).name);
console.log(two.bind(null, 1).length);
Object.defineProperty(two, "length", { value: 7 });
Object.defineProperty(two, "name", { value: "renamed" });
console.log(two.bind(null).length, two.bind(null).name);
console.log(two.bind(null, 1, 2).length);

// a length that is not a small non-negative integer still goes through
// ToIntegerOrInfinity, and +∞ survives
Object.defineProperty(two, "length", { value: 2.9 });
console.log(two.bind(null).length);
Object.defineProperty(two, "length", { value: -3 });
console.log(two.bind(null).length);
Object.defineProperty(two, "length", { value: 2147483648 });
console.log(two.bind(null).length);
Object.defineProperty(two, "length", { value: Infinity });
console.log(two.bind(null).length);

// the thisArg still reaches a receiver-reading declaration, and the
// partial args still concatenate ahead of the call's own
function tally(this: any, a: number, b: number): number {
  return (this as any).base + a + b;
}
console.log(tally.bind({ base: 100 })(1, 2));
console.log(tally.bind({ base: 100 }, 10)(2));
console.log(tally.bind({ base: 100 }, 10, 20)());

// bound-over-bound keeps the innermost target and the earliest partials
const t1 = tally.bind({ base: 0 }, 5);
const t2 = t1.bind({ base: 999 }, 6);
console.log(t2());
console.log(t2.name);
