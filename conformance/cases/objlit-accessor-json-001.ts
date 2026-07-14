// RFC 20260714-objlit-accessor — JSON.stringify serializes through
// [[Get]] (§25.5.2.4), so an accessor contributes its GETTER'S RESULT
// under the plain property name. Pre-fix the accessor slot reached the
// value recursion as a `Type::Closure` and the whole program failed to
// compile ("JSON.stringify on type Closure not yet supported").

let stored: number = 7;

// getter-only
console.log(JSON.stringify({ a: 1, get v(): number { return 2; } }));

// get + set on one property — one key, the getter's value
console.log(JSON.stringify({
  b: 1,
  get w(): number { return stored; },
  set w(x: number) { stored = x; },
}));

// setter-only — [[Get]] is undefined, so step 8.b omits the key entirely
console.log(JSON.stringify({ c: 1, set u(x: number) { stored = x; } }));
console.log(JSON.stringify({ set only(x: number) { stored = x; } }));

// the getter really runs (and sees its captured environment)
let count: number = 0;
const counted = { get hits(): number { count += 1; return count; } };
console.log(JSON.stringify(counted), JSON.stringify(counted), count);

// accessor returning a string / an object — the value recursion is the
// ordinary one, keyed off the getter's return type
console.log(JSON.stringify({ get s(): string { return "hi"; } }));
console.log(JSON.stringify({ get o(): { n: number } { return { n: 3 }; } }));

// a plain object is untouched
console.log(JSON.stringify({ x: 1, y: "z" }));
