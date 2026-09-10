// A binding that holds an `any[]` (the empty-literal collector idiom)
// takes a typed array literal on reassignment: the literal is typed
// by the slot it lands in, the same contextual typing an annotated
// declaration gives its initializer (test262 Array/length/S15.4.5.1_*).
var x = [];
x.length = true;
console.log(x.length);
x = [0];
x.length = null;
console.log(x.length);
x = [0];
x.length = new Boolean(false);
console.log(x.length);
x = [];
x.length = new Number(1);
console.log(x.length);
x = [1, 2, 3];
console.log(x.length, x[2], x.join(","));
var y = [];
function fill() { y = [4, 5]; }
fill();
console.log(y.length, y[1]);
let z: any[] = [];
z = ["s", 1];
console.log(z.length, typeof z[0]);
