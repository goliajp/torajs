// §20.4.2.2 Symbol.for step 1 is `? ToString(key)`: a Symbol key throws
// (§7.1.17 step 2) and a throwing toString propagates.
//
// bun is not an oracle here: it answers both calls without throwing
// (bun 1.4.2). node follows the spec, so this checks against a
// `.expected`.
try {
  Symbol.for(Symbol("x"));
  console.log("no throw");
} catch (e) {
  console.log(e.constructor.name);
}
try {
  Symbol.for({ toString() { throw new RangeError("r"); } });
  console.log("no throw");
} catch (e) {
  console.log(e.constructor.name);
}
