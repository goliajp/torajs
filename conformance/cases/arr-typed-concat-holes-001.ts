// concat keeps the holes of its typed receiver and array arguments
// (§23.1.3.1 step 5.c.iv); scalars and dense arrays append as before.
let a = [1.5, 2.5];
a[4] = 3.5;
console.log(a.concat([9]), a.concat());
console.log(a.concat(a));
const c = a.concat([0.5]);
console.log(2 in c, 5 in c, c.length, c.filter(() => true));
const b = [7.5];
console.log(b.concat(a));
console.log(b.concat(1.5, a));
let g = [1, 2];
g.length = 3;
console.log(g.concat([4]), [0.5].concat(g));
const t = ["x"];
t[2] = "z";
console.log(t.concat(["w"]));
const d = [1.5, 2.5];
console.log(d.concat([3.5], 4.5), d.concat());
