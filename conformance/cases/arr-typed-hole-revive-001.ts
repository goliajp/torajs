// RFC 20260721-typed-grow-on-write chunk B — an in-bounds write to a
// HOLE index on a typed block revives it as a default data property
// (§10.1.5.1); the plain-array path stays one header-bit test.
// grow-as-holes gap, then fill one hole
let x = [1, 2, 3];
x[6] = 7;
x[4] = 5;
console.log(4 in x, 3 in x, x.indexOf(5));
// length-grow holes, then write
let y = [1];
y.length = 4;
y[2] = 9;
console.log(2 in y, 1 in y);
// f64 lane
let f = [1.5];
f[3] = 4.5;
f[1] = 2.5;
console.log(1 in f, 2 in f);
// str lane — join sees the revived slot's value
let s = ["a"];
s[2] = "c";
s[1] = "b";
console.log(1 in s, s.join(","));
// any-alias lane regression guard (was already reviving)
let a2 = [1, 2];
let r: any = a2;
r[4] = 7;
r[3] = 6;
console.log(3 in a2, a2.length);
