// A rest-parameter fn-expr may still take the `this` promotion.
//
// The refusal dates from before the boxed adapter grew its recv slot:
// back then a promoted `__this` param would have eaten argv[0] out
// from under the variadic adapter's positional unbox. It no longer
// can — a recv-first body's `__this` IS argv[0] by construction
// (§10.4.4: the receiver is not an argument), the adapter drops the
// argument count by one to match, and the rest window starts after
// the fixed prefix that `__this` is now part of. Meanwhile the shape
// is the dominant test262 callback spelling — `function (...args) {
// assert.sameValue(this, undefined) }` — and it was failing to
// compile at all: "closure `__closure_0` references unknown
// identifier `__this`".

const plain = function (...args: any[]) {
  return (this === undefined ? "u" : "t") + ":" + args.length;
};

// Direct call — no receiver, so `this` is undefined.
console.log(plain(1, 2));
console.log(plain());

// An explicit receiver through call / apply, cast or not.
console.log((plain as any).call({ x: 1 }, 9));
console.log((plain as any).apply(null, [1, 2, 3]));
// (The BARE `plain.call(…)` spelling on a rest body is a separate
// member-call shape the lowering does not serve yet — registered
// residue, plan-state L3b.)

// (A binding that ALSO lands in an array-HOF callback slot stays on
// the loud refusal — registered residue, plan-state L3b.)

// Through a user HOF that direct-calls it.
function run(f: any) {
  return f(1, 2, 3);
}
console.log(run(plain));

// Reading `arguments.length` alongside the rest binding and `this`.
const counted = function (...args: any[]) {
  return arguments.length + "/" + args.length + "/" + (this === undefined ? "u" : "t");
};
console.log(counted(1, 2, 3));
console.log((counted as any).call({ y: 2 }, 4, 5));

// Fixed params ahead of the rest tail.
const mixed = function (a: number, ...rest: any[]) {
  return (this === undefined ? "u" : "t") + ":" + a + ":" + rest.length;
};
console.log(mixed(1, 2, 3));
console.log((mixed as any).call({ z: 3 }, 4, 5, 6));
console.log(mixed(9));
