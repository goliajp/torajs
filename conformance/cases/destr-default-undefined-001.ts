// RFC 20260713-generator-fn-value-substrate — destructuring defaults
// fire on `undefined`, per ES §13.15.5.3 / §13.15.5.4. The
// pre-undefined-era emission compared `=== null` (object patterns)
// or only checked length (array patterns), so any-lane absent fields
// (which read back undefined) never fired their default, and an
// explicit null wrongly would have.

// Object pattern: absent field fires, explicit null / falsy values
// do NOT (test262 obj-ptrn-id-init-skipped semantics).
function fo({ w = 1, x = 2, y = 3, z = 4 }: any) {
  return [w, x, y, z] as any;
}
const r1: any = fo({ w: null, x: 0, y: false, z: "" });
console.log(r1[0], r1[1], r1[2], r1[3]);      // null 0 false
const r2: any = fo({});
console.log(r2[0], r2[1], r2[2], r2[3]);      // 1 2 3 4

// Initializer must not evaluate when the field is present.
let fired = 0;
function count(): number { fired++; return 99; }
function fc({ k = count() }: any) { return k; }
console.log(fc({ k: 7 }), fired);              // 7 0
console.log(fc({}), fired);                    // 99 1

// Array pattern: explicit undefined element fires, past-end fires,
// present element wins.
const arr: any = [undefined, 7];
const [a = 5, b = 9, c = 11] = arr;
console.log(a, b, c);                          // 5 7 11

// Throwing initializer propagates catchably (also exercises the
// generator-ctor eager path from blade 1).
function thrower(): number { throw new Error("boom"); }
const g = function* ({ q = thrower() }: any) { yield q; };
try {
  g({});
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message); // caught: boom
}
console.log(g({ q: 42 }).next().value);        // 42

// let-position destructuring defaults share the same emission.
const src: any = { m: undefined };
const { m = "dflt", n = "also" } = src;
console.log(m, n);                             // dflt also
