// two adjacent boolean globals read only by named functions: a write
// to one must not touch the other
let a: boolean = true;
let b: boolean = true;
let c: boolean = false;
function f() { return [a, b, c]; }
a = false;
console.log(f().join(","));
b = false;
console.log(f().join(","));
c = true;
a = true;
console.log(f().join(","));
