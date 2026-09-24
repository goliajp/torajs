// ++ / -- on an any[] element (and an arguments object's index) is the
// §13.4 update: ToNumeric, +1 in its own numeric domain, stored back
// through the element write — not an add on the NaN-box bits.
function t() { arguments[1] = 7; ++arguments[1]; console.log(arguments[1], arguments.length); arguments[3] = 1; arguments[3]++; console.log(arguments[3]); arguments[5] += 2; console.log(arguments[5]); }
t();
function u(a: any) { arguments[0]++; --arguments[0]; arguments[0]--; return a + ":" + arguments[0]; }
console.log(u(5));
var xs: any[] = [1.5, "2", 7];
xs[0]++; ++xs[1]; xs[2]--; var r = xs[5]++;
console.log(xs.join(), r, xs.length);
