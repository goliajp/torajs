// ES §13.5.1.2 step 5 — a REFUSED delete throws only in strict code.
//
// OrdinaryDelete (§10.1.10) answers `false` for a non-configurable own
// property and throws nothing; the TypeError belongs to the delete
// EXPRESSION's strictness. This file is `.cts`, so sloppy, and every
// refusal below must answer `false` rather than throw. The strict twin
// is `delete-nonconfigurable-strict-001.ts`.

var o: any = {};
Object.defineProperty(o, "fixed", { value: 7, configurable: false });
Object.defineProperty(o, "loose", { value: 8, configurable: true });

// Refused: answers false, and the property survives.
console.log(delete o.fixed);
console.log(o.fixed);

// Configurable: deleted, answers true.
console.log(delete o.loose);
console.log("loose" in o);

// Computed key, same answer.
console.log(delete o["fixed"]);
console.log(o.fixed);

// Absent property: nothing to refuse, answers true.
console.log(delete o.neverThere);

// A frozen object's properties are all non-configurable.
var frozen: any = Object.freeze({ a: 1 });
console.log(delete frozen.a, frozen.a);

// The nullish-receiver TypeError is NOT the refusal, and still throws
// under both goals: §13.5.1.2 evaluates the reference first and
// ToObject on undefined throws before any [[Delete]] happens.
try {
  var nothing: any = undefined;
  console.log(delete nothing.k);
} catch (e: any) {
  console.log(e instanceof TypeError);
}
