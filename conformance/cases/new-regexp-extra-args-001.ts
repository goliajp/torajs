// §13.3.5 — `new RegExp(pattern, flags, ...extra)`: every argument is
// evaluated, in order, before the constructor runs; the extras are
// ignored. A throwing extra argument aborts the construction.
const log: string[] = [];
function t(s: string): string {
  log.push(s);
  return s;
}
const p: any = "a+";
const re = new RegExp(t("b+"), t("g"), t("x"), t("y"));
console.log(re.source, re.flags, log.join(","));
const re2 = new RegExp(p, "i", 1);
console.log(re2.test("AA"), re2.flags);
try {
  new RegExp(t("c"), t("m"), (() => { throw new RangeError("r"); })());
} catch (e) {
  console.log(e instanceof RangeError, log.join(","));
}
