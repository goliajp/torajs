function f1(a, b = 39,) { return a; }
console.log(f1.length);
function f2(a, b) { return a; }
console.log(f2.length);
function f3(...rest) { return rest; }
console.log(f3.length);
function f4(a, b = 1, c) { return a; }
console.log(f4.length);
function f5() { return 0; }
console.log(f5.length);
const g1 = function (a, b = 2) { return a; };
console.log(g1.length);
const g2 = (a = 5) => a;
console.log(g2.length);
function* g3(a, b = 1) { yield a; }
console.log(g3.length);
async function g4(a, b = 1,) { return a; }
console.log(g4.length);
function f6(a, ...rest) { return a; }
console.log(f6.length);
