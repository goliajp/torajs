// 567-02 — §10.2.9 SetFunctionName under a computed key, for the two
// value shapes 565-03 could not reach: a class expression and a
// generator expression. Both are hoisted out of the literal by the
// parser and leave a reference behind, so neither arrives at the
// field as the syntactic function definition 565-03 matched on.

const k = "c1";
const s = Symbol("d");
const o: any = {
  [k]: class {},
  [k + "z"]: function* () {
    yield 1;
  },
  [k + "a"]: async function () {},
  [s]: class {},
};
console.log(JSON.stringify(o[k].name));
console.log(JSON.stringify(o[k + "z"].name));
console.log(JSON.stringify(o[k + "a"].name));
console.log(JSON.stringify(o[s].name));

// The name is an own property with §10.2.9's attribute set, and the
// class object's `name` was already an own entry (§10.2.3
// MakeConstructor), so this is a redefine in place.
const d = Object.getOwnPropertyDescriptor(o[k], "name")!;
console.log(d.value, d.writable, d.enumerable, d.configurable);

// A value that already has a name of its own keeps it (§15.5.5 for a
// self-name, §8.4.5's "anonymous only" for everything else).
const named: any = { [k]: class Inner {}, [k + "g"]: function* gen() { yield 1; } };
console.log(JSON.stringify(named[k].name), JSON.stringify(named[k + "g"].name));

class Decl {}
const g: any = function* () {
  yield 2;
};
const refs: any = { [k]: Decl, [k + "r"]: g };
console.log(JSON.stringify(refs[k].name), JSON.stringify(refs[k + "r"].name));

// The INSPECT face stays the source's answer, which for a computed
// key is no name at all — the same split 565-03 recorded for an
// ordinary closure.
console.log(o[k], named[k]);

// The generator still generates.
const it = o[k + "z"]();
console.log(it.next().value, it.next().done);
console.log(new (o[k])() instanceof o[k]);
