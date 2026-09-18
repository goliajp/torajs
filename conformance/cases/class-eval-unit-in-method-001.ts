// K6-1 — a capture-free class expression in a method body is a unit class:
// every call of the method mints its own class object and prototype.
class Outer { make(): any { return class { who(): string { return "inner"; } }; } }
const o = new Outer();
const A: any = o.make(), B: any = o.make();
console.log(A === B, A.prototype === B.prototype);
console.log(new A() instanceof A, new A() instanceof B);
console.log(new A().who());
