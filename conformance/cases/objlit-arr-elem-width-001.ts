// A number field of an array element keeps its fraction: the element
// struct widens with the values the array's literals and pushes store.
type A = { right: number };
const xs: A[] = [{ right: 3.5 }];
console.log(xs[0].right);

const ys: A[] = [];
ys.push({ right: 2.25 });
ys.push({ right: 4 });
console.log(ys[0].right, ys[1].right, ys.length);

interface I {
  right: number;
}
const zs: I[] = [{ right: 1.5 }, { right: 2 }];
let s = 0;
for (const z of zs) s += z.right;
console.log(s, zs[0].right, zs[1].right);

// integers and fractions mixed in one array
const mixed: A[] = [{ right: 1 }, { right: -0.5 }, { right: 1e3 }, { right: 2 ** 40 }];
console.log(mixed.map((m) => m.right).join(","));

// a widened value stored beside literals reads back unchanged
const a: A = { right: 7 };
const ws: A[] = [a, { right: 3.5 }, { right: 1 }];
console.log(ws[0].right, ws[1].right, ws[2].right);
ws[0].right = 0.125;
console.log(a.right, ws[0].right);
