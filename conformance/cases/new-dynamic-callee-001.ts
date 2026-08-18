function t(f: any): string {
  try { f(); return "no-throw"; } catch (e) { var x: any = e; return x.name; }
}
console.log(t(function(): void { new (true as any)(); }));
console.log(t(function(): void { var n: any = 1; new n(); }));
console.log(t(function(): void { new ("s" as any)(); }));
var F: any = function(): void {};
var inst: any = new F();
console.log(typeof inst);
var G: any = function(): void { var s: any = this; s.v = 7; };
var g: any = new G();
console.log(g.v);
console.log(typeof new function(): void {}());
