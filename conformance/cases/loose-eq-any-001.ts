// RFC 20260713-loose-eq-substrate blade 1 — Any-side loose equality
// routes through the runtime §7.2.14 ladder.

// any × primitive
let a: any = 1;
console.log(a == 1);
console.log(a == "1");
console.log(a == true);
console.log(a != true);
console.log(1 == a);
console.log("1" == a);

// any × any cross-type
let s: any = "1";
console.log(a == s);
let f: any = false;
let z: any = 0;
console.log(f == z);
console.log(f == a);

// any × nullish
let n: any = null;
let u: any = undefined;
console.log(n == u);
console.log(n == a);
console.log(u != a);

// any(string) × number both directions
let t: any = "2.5";
console.log(t == 2.5);
console.log(2.5 == t);
console.log(t == 2);

// NaN never loose-equals anything
let nan: any = NaN;
console.log(nan == nan);
console.log(nan == 0);

// any(bigint) strict identity across distinct cells
let x: any = 1n;
let y: any = 1n;
console.log(x === y);
console.log(x == y);

// any(object) ToPrimitive via valueOf
let o: any = { valueOf: function () { return 7; } };
console.log(o == 7);
console.log(7 == o);
console.log(o == 8);
console.log(o == true);

// any(array) ToPrimitive via join
const earr: number[] = [];
let e: any = earr;
console.log(e == 0);
console.log(e == "");
let one: any = [1];
console.log(one == 1);
console.log(one == "1");
