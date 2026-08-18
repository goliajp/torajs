class C {
  get [1 + 1]() { return 42; }
  [10 * 2]() { return "m"; }
}
let c = new C();
console.log(c[1 + 1]);
console.log((c as any)[2]);
console.log((c as any)[20]());
let d: any = new C();
console.log(d[2]);
let C2 = class {
  get [1 + 1]() { return 2; }
  set [1 + 1](v: any) { }
  static get [1 + 1]() { return 2; }
  static set [1 + 1](v: any) { }
};
let c2 = new C2();
console.log(c2[1 + 1]);
console.log((c2[1 + 1] = 2));
console.log(C2[1 + 1]);
console.log((C2[1 + 1] = 2));
