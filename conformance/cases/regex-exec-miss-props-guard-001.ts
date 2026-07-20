// rotation 169 — typed-lane exec/match miss (null) named-prop reads
// (`m.groups` / `m.index`) must throw a catchable TypeError, not
// SIGSEGV (test262 duplicate-named-groups-properties exit 139 pair).
const miss = /zzz/.exec("abc");
try {
  console.log(miss.groups);
} catch (e) {
  console.log("caught groups");
}
try {
  console.log(miss.index);
} catch (e) {
  console.log("caught index");
}
const missMatch = "abc".match(/zzz/);
try {
  console.log(missMatch.groups);
} catch (e) {
  console.log("caught match groups");
}
// hit path keeps answering the attached props
const hit = /(?<w>b)/.exec("abc");
if (hit !== null) {
  console.log(hit.index);
  console.log(hit.input);
  console.log(hit.groups.w);
}
console.log("done");
