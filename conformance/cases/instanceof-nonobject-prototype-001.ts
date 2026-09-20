// §7.3.22 OrdinaryHasInstance step 6 — "If P is not an Object, throw a
// TypeError exception". tr ran step 5's Get and then dropped the pair's
// tag on the floor, so a non-object `.prototype` walked the chain looking
// for a cell at the primitive's address, never found it, and answered
// false where the spec raises.

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + (e as any).constructor.name);
  }
}

function mk(p: unknown): any {
  const f: any = function () {};
  f.prototype = p;
  return f;
}

probe("proto-true", () => ({}) instanceof mk(true));
probe("proto-number", () => ({}) instanceof mk(1));
probe("proto-string", () => ({}) instanceof mk("a long string not stored inline"));
probe("proto-symbol", () => ({}) instanceof mk(Symbol("s")));
probe("proto-bigint", () => ({}) instanceof mk(10n));
probe("proto-null", () => ({}) instanceof mk(null));

// `f.prototype = undefined` is NOT here, and the reason is worth writing
// down: the closure props dict spells "absent" as tag 5, which is also
// the tag an explicit `undefined` carries, so `fn_prototype_pair` cannot
// tell the two apart and takes the absent branch — it mints a fresh
// prototype object and the walk answers false. Distinguishing them is a
// change to that sentinel, not to this step, so the case stays open
// rather than being claimed here.

// Controls — an object prototype still answers, both ways.
const g: any = function () {};
const inst: any = new g();
probe("proto-object-hit", () => inst instanceof g);
probe("proto-object-miss", () => ({}) instanceof g);
probe("proto-array", () => ({}) instanceof mk([1, 2]));
probe("proto-function", () => ({}) instanceof mk(function () {}));

// Control — a non-object LEFT side is step 3, which answers false and
// does not throw, so the two steps must not be confused.
probe("lhs-primitive", () => (1 as any) instanceof g);
probe("lhs-primitive-bad-proto", () => (1 as any) instanceof mk(true));

// Control — the ordinary builtin spelling is untouched.
console.log([] instanceof Array, ({}) instanceof Object, /a/ instanceof RegExp);

// The class lane asks the same step. tr models a class as a dynobj, so
// the closure walk above cannot read its `.prototype` and a separate arm
// handles it — that arm answered "not a class object", whose caller
// raises "Right-hand side of 'instanceof' is not callable", and only in
// strict mode. Wrong error, and in sloppy code no error at all.
class K {}
function mkc(p: unknown): any {
  const c: any = K;
  (c as any).prototype = p;
  return c;
}
probe("class-proto-true", () => ({}) instanceof mkc(true));
probe("class-proto-number", () => ({}) instanceof mkc(7));
