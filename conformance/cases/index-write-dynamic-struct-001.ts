class C {
  x: number = 1;
}
let c = new C();
let k = 5;
c[k] = 42;
console.log((c as any)[5], c.x);
let o = { 0: "x", 1: "y" };
let i = 1;
o[i] = "z";
console.log(o[1], o[0]);
