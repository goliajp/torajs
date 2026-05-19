// P7.4-frozen — assigning to a property of a frozen object throws a
// real catchable TypeError (spec §10.1.5 OrdinarySet: strict-mode
// assignment to a non-writable property), NOT a process abort, and
// the illegal mutation is prevented (the throw-check diverts before
// the field store). Asserts spec-defined facts only: instance type,
// prototype chain, .name, catchability, and that the value is
// unchanged. Propagation shape: direct try/catch (cross-named-fn
// propagation of property-assign throws is a separate pre-existing
// substrate boundary, same as a-2's dynobj writable=false path).

class Box { v: number = 1; }

const o = new Box();
Object.freeze(o);
try {
  o.v = 99;
  console.log("BUG: frozen assign did not throw");
} catch (e: TypeError) {
  console.log(
    "frozen | " +
    (e instanceof TypeError) + " | " +
    (e instanceof Error) + " | " +
    e.name,
  );
}
// mutation must have been prevented
console.log("o.v=" + o.v);

// unfrozen object still mutable (no spurious throw)
const o2 = new Box();
o2.v = 42;
console.log("o2.v=" + o2.v);

// a second frozen object, distinct instance
const f2 = new Box();
Object.freeze(f2);
try {
  f2.v = 7;
  console.log("BUG: f2 frozen assign did not throw");
} catch (e: TypeError) {
  console.log("frozen2 | " + (e instanceof TypeError) + " | " + e.name);
}
console.log("f2.v=" + f2.v);
