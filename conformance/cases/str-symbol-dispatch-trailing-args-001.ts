// String.prototype.match / matchAll / search read only their first
// argument (§22.1.3.12, §22.1.3.14, §22.1.3.20), so a trailing
// argument changes neither the @@match / @@matchAll / @@search
// dispatch nor the result's type; the trailing expressions still run.

var trail: string[] = [];
function t(tag: string): number {
  trail.push(tag);
  return 0;
}

var o: any = {};
o[Symbol.match] = function (x: string) {
  return 42;
};
// matchAll's step 2.a IsRegExp reads @@match, so its pattern is a
// separate object
var oa: any = {};
oa[Symbol.matchAll] = function (x: string) {
  return "MA:" + x;
};
o[Symbol.search] = function (x: string) {
  return null;
};

const m = "abc".match(o, t("m"));
console.log(m === null, m + 1);
const ma: any = "abc".matchAll(oa, t("ma"));
console.log(ma);
const se = "abc".search(o, t("se"));
console.log(se === null, typeof se);

function viaReturn() {
  return "abc".match(o, 1, 2);
}
console.log(viaReturn());

// without a custom method the coerce lane answers, trailing args or not
var p: any = "b";
const pm = "abcb".match(p, t("pm"));
console.log(pm === null ? "null" : pm[0]);
console.log("abcb".search(p, t("ps")), "abc".search(p));
var miss: any = "z";
console.log("abc".match(miss, 0) === null, "abc".search(miss, 0));
console.log(trail.join(","));
