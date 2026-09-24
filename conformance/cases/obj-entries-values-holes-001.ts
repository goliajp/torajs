// §7.3.24 EnumerableOwnProperties walks OwnPropertyKeys, which holds
// no key for a hole — Object.entries / Object.values over an array
// with holes list only the present indices.
const a: any = new Array(4);
a[1] = "one";
a[3] = 3;
console.log(JSON.stringify(Object.entries(a)), JSON.stringify(Object.values(a)), Object.keys(a).join());
const b: any = [0, , 2, , 4];
console.log(JSON.stringify(Object.entries(b)), JSON.stringify(Object.values(b)));
const c: any = [1, 2, 3];
delete c[0];
c.extra = "e";
console.log(JSON.stringify(Object.entries(c)), JSON.stringify(Object.values(c)));
const d: any = [];
d.length = 3;
d[2] = "z";
console.log(JSON.stringify(Object.entries(d)), JSON.stringify(Object.values(d)));
