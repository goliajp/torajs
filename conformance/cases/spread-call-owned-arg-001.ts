// A fresh argument next to a spread is pushed into the argument list
// and released once: the callee still reads it, and nothing frees it
// twice.
class O {
  m(...a: any[]) {
    return a.join("|");
  }
}
const o = new O();
const d: any = new O();
const xs = [1, 2];
const ys: any[] = ["p", { k: 1 }];
function f(...a: any[]) {
  return a.length + ":" + a[a.length - 1];
}
function mk(n: number): any {
  return { n: n };
}
let last = "";
for (let i = 0; i < 3; i++) {
  last = o.m(...xs, "k" + i) + " " + d.m(...xs, "d" + i) + " " + f(...xs, "f" + i);
}
console.log(last);
console.log(o.m(...ys, "a" + 1, ...xs, [3, 4].join("-")));
console.log(f(...xs, mk(5)).length, f(...ys, mk(6), "z" + 2));
const kept = "s" + xs.length;
console.log(o.m(...xs, kept), kept);
const arr: any[] = [...xs, "x" + 9, mk(7)];
console.log(arr.length, arr[2], arr[3].n);
