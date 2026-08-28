// §22.1.3.x String.prototype.{match,search,replace,split} step 3.a is
// GetMethod(pattern, @@sym) — ONE Get. tr asked twice: a presence
// probe the SSA branched on, then an invoke that walked the symbol
// again. A data property cannot tell the difference; an accessor can.
// The probe saw only the ACCESSOR sentinel — neither undefined nor
// null, so it reported "present" — and the invoke's second walk got
// the same sentinel and reported "not a function". The getter never
// ran, and there was no way to reach the verdict a getter can produce:
// "it answered nullish, so use the step-4 coerce lane".

// each face, accessor-shaped
const m: any = {};
Object.defineProperty(m, Symbol.match, {
  get() { console.log("GET match"); return (s: string) => "M:" + s; },
});
console.log("abc".match(m)); // GET match / M:abc

const s1: any = {};
Object.defineProperty(s1, Symbol.search, { get() { return (_s: string) => 42; } });
console.log("abc".search(s1)); // 42

const r1: any = {};
Object.defineProperty(r1, Symbol.replace, {
  get() { return (s: string, rep: string) => "R:" + s + ":" + rep; },
});
console.log("abc".replace(r1, "x")); // R:abc:x

const p1: any = {};
Object.defineProperty(p1, Symbol.split, {
  get() { return (s: string, lim: any) => ["S", s, String(lim)]; },
});
console.log("a,b".split(p1, 7).join("|")); // S|a,b|7

// exactly one getter run per call — the spec's single GetMethod
const m2: any = {};
let n = 0;
Object.defineProperty(m2, Symbol.match, { get() { n++; return (s: string) => "M:" + s; } });
console.log("abc".match(m2), n); // M:abc 1
console.log("abc".match(m2), n); // M:abc 2

// a throwing getter propagates instead of being reported as
// "not a function"
const m3: any = {};
Object.defineProperty(m3, Symbol.replace, { get() { throw new Error("boom"); } });
try {
  console.log("abc".replace(m3, "x"));
} catch (e: any) {
  console.log("caught", e.message);
} // caught boom

// a getter answering nullish means "no method" — the step-4 coerce
// lane runs, which the probe/invoke split could never reach
const m4: any = {};
Object.defineProperty(m4, Symbol.split, { get() { console.log("GET nullish"); return undefined; } });
console.log("a-b-c".split(m4).join("|")); // GET nullish / a-b-c

// a getter answering a non-callable still refuses (after running)
const m5: any = {};
Object.defineProperty(m5, Symbol.search, { get() { console.log("GET bad"); return 5; } });
try {
  console.log("abc".search(m5));
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
} // GET bad / caught true

// inherited accessor and object-literal getter syntax
const proto: any = {};
Object.defineProperty(proto, Symbol.search, { get() { return (_s: string) => 7; } });
const inh: any = Object.create(proto);
console.log("abc".search(inh)); // 7
const m6: any = { get [Symbol.match]() { return (s: string) => "L:" + s; } };
console.log("abc".match(m6)); // L:abc

// data-property and builtin RegExp forms are untouched
const m7: any = { [Symbol.match]: (s: string) => "D:" + s };
console.log("abc".match(m7)); // D:abc
console.log("a1b2".replace(/\d/, "#")); // a#b2
console.log("a1b2".split(/\d/).join("|")); // a|b|
console.log("a1b2".search(/\d/)); // 1
console.log("abc".match(/b/)![0]); // b
