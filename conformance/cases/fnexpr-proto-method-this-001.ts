// F.prototype method assigned as fn-expr: body this = call receiver
function E(this: any) { this.x = 5; }
E.prototype.m = function() { return this.x * 2; };
E.prototype.tag = function(s: string) { return s + ":" + this.x; };
var e = new (E as any)();
console.log((e as any).m(), (e as any).tag("v"));
function It(this: any, n: number) { this.i = 0; this.n = n; }
It.prototype.next = function() {
  if (this.i >= this.n) { return { done: true, value: 0 }; }
  return { value: this.i++, done: false };
};
It.prototype[Symbol.iterator] = function() { return this; };
var acc: number[] = [];
for (var v of (new (It as any)(3)) as any) { acc.push(v); }
console.log(acc.join(","));
var proto: any = (E as any).prototype;
console.log(proto.m.call ? "callable" : "?", (new (E as any)() as any).m());
