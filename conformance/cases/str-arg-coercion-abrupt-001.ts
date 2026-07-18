// §22.1.3 argument-coercion order: each ? ToString / ? ToIntegerOrInfinity
// aborts (ReturnIfAbrupt) — a later coercion's throw must not clobber an
// earlier one's.
const throwStr = (id: string): any => ({
  valueOf: function (): any { return {}; },
  toString: function (): any { throw id; },
});
const throwNum = (id: string): any => ({
  valueOf: function (): any { throw id; },
});
function probe(label: string, run: () => unknown): void {
  try {
    run();
    console.log(label, "no-throw");
  } catch (e) {
    console.log(label, e);
  }
}
const s: any = "ABBABABAB";
probe("slice", () => s.slice(throwStr("instart"), throwNum("inend")));
probe("substring", () => s.substring(throwStr("instart"), throwNum("inend")));
probe("substr", () => s.substr(throwStr("instart"), throwNum("inend")));
probe("indexOf", () => s.indexOf(throwStr("intostr"), throwNum("intoint")));
probe("includes", () => s.includes(throwStr("intostr"), throwNum("intoint")));
probe("lastIndexOf", () => s.lastIndexOf(throwStr("intostr"), throwNum("intoint")));
probe("startsWith", () => s.startsWith(throwStr("intostr"), throwNum("intoint")));
probe("endsWith", () => s.endsWith(throwStr("intostr"), throwNum("intoint")));
probe("padStart", () => s.padStart(throwNum("inlen"), throwStr("infill")));
probe("padEnd", () => s.padEnd(throwNum("inlen"), throwStr("infill")));
probe("slice-2nd", () => s.slice(1, throwNum("inend")));
console.log("plain", s.slice(1, 4), s.indexOf("B", 2), s.padStart(11, "xy"));
