// Own entries storing undefined shadow the prototype in
// OrdinaryToPrimitive and method dispatch (§7.1.1.1 / §13.3.6.2):
// dynobj get_tag conflates "absent" with "own entry storing
// undefined", so the inherited Object.prototype surface must not
// leak through an own undefined entry (test262 trimStart/trimEnd
// this-value-object valueOf-err cluster).

// valueOf fallback when own toString stores undefined
const onlyV: any = {
  toString: undefined,
  valueOf: function () {
    return "V2";
  },
};
console.log(String(onlyV)); // V2
console.log("" + onlyV); // V2

// poisoned valueOf accessor fires — and its throw stays catchable —
// once the undefined toString is skipped (TrimString's ToString)
const poisoned: any = {
  toString: undefined,
  get valueOf() {
    throw new Error("boom");
  },
};
let caught = false;
try {
  String.prototype.trimEnd.call(poisoned);
} catch (e) {
  caught = true;
}
console.log(caught); // true

// a method call through an own undefined entry answers the
// resolved-not-callable TypeError, not the proto method
const m: any = { toString: undefined };
let threw = false;
try {
  m.toString();
} catch (e) {
  threw = true;
}
console.log(threw); // true
console.log("done");
