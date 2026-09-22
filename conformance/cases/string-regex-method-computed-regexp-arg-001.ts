// A RegExp-typed argument that is not a literal or a binding —
// `new RegExp(p)` with a non-literal `p`, or a call returning a RegExp —
// is the regex itself (§22.1.3.11 step 2 hands it to its own @@match),
// not a value to ToString into a new pattern.
const s = "ab0c12,3b0";
const p: any = "b0";
const q: any = ["c1"];
function mk(): RegExp {
  return new RegExp("[0-9]", "g");
}
console.log(s.match(new RegExp(p)) !== null, s.match(new RegExp(q))?.[0]);
console.log(s.search(new RegExp(q)), s.replace(new RegExp(p, "g"), "_"));
console.log(s.split(new RegExp(p)), s.match(mk()));
console.log([...s.matchAll(new RegExp(p, "g"))].length);
for (let i = 0; i < 3; i++) {
  console.log(s.replace(new RegExp(q), "Q" + i));
}
