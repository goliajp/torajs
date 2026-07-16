// StringWrapper §10.4.3.3 OwnPropertyKeys — the [[StringData]]
// integer indices are own enumerable properties (keys/values/entries)
// and gOPN additionally lists "length"; Number/Boolean wrappers have
// no inherent own keys.
function mks(v: any): any {
  return new String(v);
}
function mkn(v: any): any {
  return new Number(v);
}
const w = mks("ab");
console.log(Object.keys(w));
console.log(Object.getOwnPropertyNames(w));
console.log(Object.values(w));
console.log(Object.entries(w));
const n = mkn(5);
console.log(Object.keys(n), Object.values(n));
const e = mks("");
console.log(Object.keys(e), Object.getOwnPropertyNames(e));
