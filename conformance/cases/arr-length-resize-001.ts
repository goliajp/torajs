// arr.length real resize — §10.4.2.5 ArraySetLength (assign + define
// lanes; truncate releases slots, Array<Any> grow fills undefined)
const a: any[] = [1, 2, 3, 4, 5];
a.length = 3;
console.log(a.length, a[0], a[2], a[3]);
a.length = 5;
console.log(a.length, a[3], a[4]);
Object.defineProperty(a, "length", { value: 2 });
console.log(a.length, a[1], a[2]);
Object.defineProperty(a, "length", { value: null });
console.log(a.length);
Object.defineProperty(a, "length", { value: "3" });
console.log(a.length, a[0]);
// refcounted elements release on truncate (churn probe covers rc)
const s: any[] = ["aa", "bb", "cc"];
s.length = 1;
console.log(s.length, s[0], s[1]);
// grow then write into the new range
const g: any[] = [7];
g.length = 4;
g[3] = 9;
console.log(g.length, g[0], g[1], g[3]);
// invalid values -> RangeError
try {
  a.length = -1;
  console.log("no-throw");
} catch (e) {
  console.log("neg", e instanceof RangeError);
}
try {
  Object.defineProperty(a, "length", { value: 1.5 });
  console.log("no-throw");
} catch (e) {
  console.log("frac", e instanceof RangeError);
}
const u: any = undefined;
try {
  a.length = u;
  console.log("no-throw");
} catch (e) {
  console.log("undef", e instanceof RangeError);
}
console.log(a.length);
// typed scalar array truncate regression (old lane)
const t = [10, 20, 30];
t.length = 1;
console.log(t.length, t[0]);
