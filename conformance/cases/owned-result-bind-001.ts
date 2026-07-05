// RFC 20260705 owned-result invariant: let-binding a chaining/borrow
// method result must not double-drop the receiver cell (pre-fix:
// b and a each dropped the same pointer = silent UAF).
let a = [3, 1, 2];
let b = a.reverse();
console.log(b[0]);
console.log(a[0]);
let c = a.sort();
console.log(c[0]);
let d = a.fill(9, 0, 1);
console.log(d[0]);
let e = a.copyWithin(0, 1);
console.log(e[0]);
let s = "xy";
let t = s.concat();
console.log(t);
let strs = ["p", "q"];
let atv = strs.at(1);
console.log(atv);
let vo = strs.valueOf();
console.log(vo[0]);
// inner-scope binding drop while outer binding stays live
let big = ["aa", "bb", "cc"];
{
  let inner = big.reverse();
  console.log(inner[0]);
}
console.log(big[0]);
console.log(big[2]);
