// RFC 20260721-string-proto-cluster 刀 3 — per-(family, mid) method
// cells + builtin-proto monkey-patch consult. String.prototype
// methods borrowed onto other receivers run the §22.1.3 generic
// ToString(this); patches on builtin prototype singletons resolve
// on dispatch miss; distinct prototypes' same-name methods are
// distinct function objects.
(Number.prototype as any).split = String.prototype.split;
let n: any = new Number(100111122133144155);
let r = n.split(1);
console.log(r.length, r.join("|"));
console.log(r.constructor === Array);
let b: any = new Boolean;
b.indexOf = String.prototype.indexOf;
console.log(b.indexOf("false"));
b.concat = String.prototype.concat;
console.log(b.concat("A", true, 2));
(String.prototype as any).myfn = function () {
  return "custom";
};
let s: any = "ab";
console.log(s.myfn());
(Number.prototype as any).myfn2 = function () {
  return "n";
};
let x: any = 5;
console.log(x.myfn2());
let nw: any = new Number(7);
console.log(nw.myfn2());
console.log((String.prototype.concat as any) === (Array.prototype.concat as any));
console.log([1, 2, 3].indexOf(2));
console.log(("abc" as any).indexOf("b"));
console.log(("abc" as any).at(1));
console.log(([10, 20] as any).at(1));
