var R = (function () {
  return { tag: 7 };
}) as any;
var BR = R.bind(null);
console.log(new BR().tag);
var S = (function (a: any, b: any) {
  return { sum: a + b };
}) as any;
var BS = S.bind(null, 1);
console.log(new BS(2).sum);
var BSS = BS.bind(null, 2);
console.log(new BSS().sum);
var F = (function () {}) as any;
var BF = F.bind(null);
var o = new BF();
(o as any).x = 5;
console.log((o as any).x);
var A = ((): number => 1) as any;
var BA = A.bind(null);
try {
  new BA();
} catch (e) {
  console.log("caught");
}
