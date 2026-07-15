// RFC 20260716 刀 21 — String.prototype.replace ToString step order
// (spec §22.1.3.15 step 4 → step 6a: ToString(searchValue) fires
// BEFORE ToString(replaceValue)). Mirrors test262
// S15.5.4.11_A1_T12 shape: `__str` is `new String(...)` so the
// call routes through the Any-method-call REPLACE arm, where the
// prior "repl first" ordering evaluated `ToString(replaceValue)`
// before `ToString(searchValue)` and, when both user-toString
// methods threw, clobbered the earlier pending throw.
const searchObj: any = {
  toString() { return {}; },
  valueOf() { throw "insearchValue"; },
};
const replaceObj: any = {
  toString() { throw "inreplaceValue"; },
};
const __str = new String("ABBABABAB");
try {
  const x = (__str as any).replace(searchObj, replaceObj);
  console.log("NO_THROW:", x);
} catch (e) {
  console.log("caught:", e);
}
