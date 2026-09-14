// §23.1.3.23 Array.prototype.push with no arguments appends nothing
// and answers the length. The typed arm handled exactly one argument
// and the generic lane had no shape for the empty list (test262
// S15.4.4.7_A1_T1 and siblings, rotation 651).
var x = new Array();
console.log(x.push(1));
console.log(x.push(), x.length, x[0]);
const ys: number[] = [];
console.log(ys.push(), ys.push(5), ys.push(), JSON.stringify(ys));
const zs: string[][] = [["a"]];
console.log(zs[0].push(), zs[0].length);
function f(arr: number[]) { return arr.push(); }
console.log(f([1, 2, 3]));
