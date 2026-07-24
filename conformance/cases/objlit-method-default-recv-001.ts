// Member-arm default padding, receiver-precise: b's OWN m has no
// default, so b.m() must NOT be padded with the unrelated a-m's
// default (the name-keyed table used to pad every x.m() site). The
// default is an effectful call so a wrongly padded site is visible
// as the side effect firing — without depending on the missing-arg
// value repr.
let padded = false;
function mark(): number {
  padded = true;
  return 5;
}
const a = { m(x = mark()) { return x; } };
const b = { m(x: any) { return 0; } };
console.log(b.m());
console.log(padded);
console.log(a.m());
console.log(padded);
console.log(a.m(1));
