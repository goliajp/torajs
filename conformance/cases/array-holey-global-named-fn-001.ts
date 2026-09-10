// §13.2.4 — an array literal with an elision is an Array<Any> with a
// hole, and a top-level binding holding one is visible to named
// function bodies like any other array global: the test262
// Array.prototype.{every,filter,forEach,map,reduce,reduceRight,some}
// "considers elements added after the call" shape.
var arr = [1, 2, , 4, 5];
function fill() { arr[2] = 3; return arr[2]; }
console.log(fill(), arr.length, arr.join(","), 2 in arr);
var calledForThree = false;
function cb(val: any, idx: number, obj: any) { holey[2] = 3; if (val == 3) calledForThree = true; return true; }
var holey = [1, 2, , 4, 5];
holey.every(cb);
console.log(calledForThree);
var red = [1, 2, , 4, "5"];
function acc(prev: any, cur: any, idx: number, obj: any) { red[5] = 6; red[2] = 3; return prev + cur; }
console.log(red.reduce(acc, ""), red.length, red.join(","));
var brr = [1, , 3];
function len() { return brr.length + ":" + (1 in brr) + ":" + brr[1]; }
console.log(len());
