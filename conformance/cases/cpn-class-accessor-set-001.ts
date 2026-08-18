class C {
  set [1 + 1](v: any) { console.log("setter", v); }
  get [1 + 1]() { return 7; }
}
let c: any = new C();
c[2] = 5;
console.log(c[2]);
let d = new C();
d[1 + 1] = 6;
console.log(d[1 + 1]);
d["2"] = 8;
console.log(d[2]);
class G {
  get [1 + 1]() { return 1; }
}
let g: any = new G();
try {
  g[2] = 9;
  console.log("wrote", g[2]);
} catch (e: any) {
  console.log("threw", e.constructor.name);
}
