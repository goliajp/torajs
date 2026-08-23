// A builtin instance sees the symbol-keyed properties its prototype
// carries — both the ones the spec installs and the ones a program
// adds.
//
// The inherited-symbol walk used to name three tags (Arr, Closure,
// the wrappers) and answer "nothing inherited" for every other
// builtin cell. So `Map.prototype[Symbol.toStringTag]` said "Map"
// while `new Map()[Symbol.toStringTag]` said undefined: the property
// was installed, and simply unreachable from an instance. The family
// map already knew every row; only this walk was not asking it.
//
// Note the two questions this file does NOT conflate.
// `Object.prototype.toString.call(x)` has answered the right badge
// all along — it walks its own ladder. What was missing is the
// PROPERTY, which is what a program reads and what
// getOwnPropertyDescriptor reports.

const sym: any = Symbol("probe");

// §20.4.3.5 / §21.2.3.5 / §27.2.5.5 / §24.1.3.14 / §24.2.3.13 /
// §24.3.3.4 / §24.4.3.5 / §26.1.3.3 / §25.1.6.5 — the nine
// prototypes the spec gives a plain data @@toStringTag.
const m: any = new Map();
const s: any = new Set();
const wm: any = new WeakMap();
const ws: any = new WeakSet();
const p: any = Promise.resolve(1);
const bi: any = 10n;
const sy: any = Symbol("x");
const ab: any = new ArrayBuffer(4);

console.log("map", m[Symbol.toStringTag], Map.prototype[Symbol.toStringTag]);
console.log("set", s[Symbol.toStringTag], Set.prototype[Symbol.toStringTag]);
console.log("weakmap", wm[Symbol.toStringTag]);
console.log("weakset", ws[Symbol.toStringTag]);
console.log("promise", p[Symbol.toStringTag]);
console.log("bigint", bi[Symbol.toStringTag]);
console.log("symbol", sy[Symbol.toStringTag]);
console.log("arraybuffer", ab[Symbol.toStringTag]);

// The badge and the property agree now.
console.log(
  "badges",
  Object.prototype.toString.call(m),
  Object.prototype.toString.call(p),
  Object.prototype.toString.call(ab),
);

// The families the spec gives NO tag still answer undefined — the
// badge comes from the builtinTag walk there, and installing one
// would be wrong rather than redundant.
const arr: any = [1];
const str: any = "ab";
const num: any = 5;
const bool: any = true;
const dt: any = new Date();
const re: any = /x/;
const obj: any = {};
console.log(
  "no-tag",
  String(arr[Symbol.toStringTag]),
  String(str[Symbol.toStringTag]),
  String(num[Symbol.toStringTag]),
  String(bool[Symbol.toStringTag]),
  String(dt[Symbol.toStringTag]),
  String(re[Symbol.toStringTag]),
  String(obj[Symbol.toStringTag]),
);

// A monkey-patch on a builtin prototype reaches its instances, which
// is the same walk under a different key.
Object.defineProperty(Map.prototype, sym, { value: 42 });
Object.defineProperty(Set.prototype, sym, { value: 43 });
Object.defineProperty(Promise.prototype, sym, { value: 44 });
Object.defineProperty(Array.prototype, sym, { value: 45 });
Object.defineProperty(Date.prototype, sym, { value: 46 });
console.log("patched", m[sym], s[sym], p[sym], arr[sym], dt[sym]);

// An own symbol entry shadows the inherited one. The patch above is
// non-writable, so the shadow needs its own defineProperty rather
// than an assignment — an inherited read-only data property refuses
// a plain write (§10.1.9.1 step 3.b).
//
// The receiver here is an Array rather than a Map on purpose: a Map
// cell has no own expando dict at all today, so a symbol-keyed
// define on one is silently dropped and the inherited entry keeps
// answering. That is a gap in the OWN half of the symbol walk, one
// layer under this file's subject, and it is recorded rather than
// asserted here.
const own: any = [1];
Object.defineProperty(own, sym, { value: 99 });
const fresh: any = [1];
console.log("shadow", own[sym], fresh[sym]);

// Two instances of the same family see the same inherited function
// object — the walk answers the prototype's entry, not a per-receiver
// mint.
const s2: any = new Set();
console.log("stable", s[Symbol.iterator] === s2[Symbol.iterator]);

// The prototype object itself is not its own instance: reading the
// key on it goes through its OWN dict, and that is where the entry
// lives.
console.log("proto-own", Map.prototype[sym]);
