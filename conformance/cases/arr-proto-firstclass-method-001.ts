// RFC 20260713-array-proto-residual blade 2 — first-class builtin
// method values + own-expando call dispatch:
// - Object.prototype.toString reifies as the distinct §20.1.3.6
//   badge classifier (proto alias; never the receiver's toString)
// - an own arr-props / dynobj entry shadows the builtin surface and
//   invokes with the receiver bound (reified cell or user closure)
var obj: any = {};
obj.slice = Array.prototype.slice;
obj[0] = 0; obj[1] = 1; obj[2] = 2;
obj.length = 3;
var arr = obj.slice(0, 3);
(arr as any).getClass = Object.prototype.toString;
console.log((arr as any).getClass());
var plain: any = [1, 2];
plain.ucb = function () { return "user-cb"; };
console.log(plain.ucb());
var o: any = { x: 1 };
o.gc = Object.prototype.toString;
console.log(o.gc());
const ots: any = Object.prototype.toString;
console.log(typeof ots);
console.log(Object.prototype.toString.call([1]));
console.log(Object.prototype.toString.call("s"));
console.log(Object.prototype.toString.call(5));
console.log(Object.prototype.toString.call(true));
console.log(Object.prototype.toString.call(null));
console.log(Object.prototype.toString.call(undefined));
console.log(Object.prototype.toString.call(new Date(0)));
console.log(Object.prototype.toString.call(/x/));
console.log(Object.prototype.toString.call(function () {}));
console.log(Object.prototype.toString.call({}));
