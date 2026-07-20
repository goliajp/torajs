// RFC 20260721-builtin-method-reflection 刀 5 — Date.prototype
// .toTimeString (§21.4.4.42): the time half of toString with the
// TZif offset + CLDR zone long name, typed + any lanes, Invalid
// Date sentinel, and the name/length reflection face.
const d = new Date(0);
console.log(d.toTimeString());
const dAny: any = new Date(0);
console.log(typeof dAny.toTimeString);
const s: any = dAny.toTimeString();
if (s === d.toTimeString()) {
  console.log("lanes-agree");
} else {
  console.log("lanes-DIFF");
}
console.log(new Date(NaN).toTimeString());
const f: any = dAny.toTimeString;
console.log(f.name, f.length);
