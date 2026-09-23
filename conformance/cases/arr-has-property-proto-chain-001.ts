// §7.3.11 HasProperty on an Array continues past own: a hole is filled
// by %Array.prototype% or %Object.prototype% digit keys.
(Object.prototype as any)[1] = 4;
const a = [1, , 3];
console.log(a[1], 1 in a);
const b: any = [1, , 3];
console.log(b[1], 1 in b);
(Array.prototype as any)[5] = "ap";
const x: any = [0];
x.length = 6;
x.reverse();
console.log(x[0], x[5], 0 in x, x.hasOwnProperty(0));
