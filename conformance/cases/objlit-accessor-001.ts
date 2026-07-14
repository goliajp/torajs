// RFC 20260714-objlit-accessor blade 2 — object-literal getters and
// setters actually run.
//
// The parser used to walk a `get b() { ... }` body brace-balanced and
// THROW IT AWAY, leaving a `__getter_b: null` placeholder field. The
// accessor never ran, and even a direct read failed to compile: "no
// member `.b` on Struct([("__getter_b", Null)])". Class getters and
// `Object.defineProperty` accessors both worked — an object literal was
// the one leg with nothing under it.
//
// The accessor closure now lives in the layout under `__getter_<n>` /
// `__setter_<n>` and is invoked with the receiver through blade 1's
// `(__env, __this, ...user)` ABI. Keeping it IN the layout is what makes
// it belong to the type: `{a:1, get b(){}}` is structurally distinct
// from `{a:1}`, so nothing with a matching shape can reach for it.

// getter reading through `this`
const a = { x: 1, get b() { return this.x + 41; } };
console.log(a.b);

// getter that CAPTURES instead of using `this`, and is called twice —
// the shape test262's `obj-ptrn-rest-getter` cases are built on
// (`{ get v() { count++; return 2; } }`). An accessor takes the receiver
// even when its body never says `this`, so this must not lose an arg.
let count = 0;
const src = { get v() { count = count + 1; return 2; } };
console.log(src.v, count);
console.log(src.v, count);

// setter
let stored = 0;
const c = { set s(n: number) { stored = n * 2; } };
c.s = 21;
console.log(stored);

// getter + setter pair on one property, round-tripped
let held = 0;
const d = {
  get p(): number { return held; },
  set p(n: number) { held = n; },
};
d.p = 7;
console.log(d.p);
d.p = d.p + 5;
console.log(d.p, held);

// accessors coexisting with data fields and a method
const e = {
  n: 10,
  get dbl() { return this.n * 2; },
  tripled() { return this.n * 3; },
};
console.log(e.n, e.dbl, e.tripled());

// a string-valued accessor, and one whose getter reads another accessor
const f = {
  first: "ada",
  last: "lovelace",
  get full() { return this.first + " " + this.last; },
  get shout() { return this.full + "!"; },
};
console.log(f.full);
console.log(f.shout);

// accessor on a literal that crosses a function return boundary
function mkCell(start: number) {
  let held2 = start;
  return {
    get value() { return held2; },
    set value(n: number) { held2 = n * 10; },
  };
}
const cell = mkCell(3);
console.log(cell.value);
cell.value = 4;
console.log(cell.value);
