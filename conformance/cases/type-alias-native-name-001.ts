// A program may declare a type named `f64` or `i64`: an alias of that
// alias resolves to what the program declared, whatever order the
// aliases are inlined in.

type f64 = string;
type A = f64;
const a: A = "x";
const b: f64 = "y";
console.log(a + b, a.length);

type i64 = number;
type B = i64;
let c: B = 0;
c += 1.5;
console.log(c);
