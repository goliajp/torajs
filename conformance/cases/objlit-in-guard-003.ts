// `in` for a declared member a cell lacks, where only the class or the
// root could answer: a class instance after a delete, and literals
// while %Object.prototype% holds only builtins, then after it grows.
class C {
  a?: number = 1;
  b = 2;
  m(): number {
    return this.b;
  }
}
const c = new C();
delete c.a;
console.log("a" in c, "m" in c, "b" in c, c.m());
type P = { a?: number; b: number };
const xs: P[] = [{ b: 1 }, { a: 1, b: 2 }, { b: 3 }];
let n = 0;
for (const x of xs) if ("a" in x) n = n + 1;
console.log(n, Object.getPrototypeOf(xs[0]) === Object.prototype);
const proto = Object.prototype as any;
proto.z = 1;
console.log("a" in xs[0], "z" in proto, "a" in c);
proto.a = 2;
console.log("a" in xs[0], "a" in c, "a" in xs[1]);
delete proto.a;
delete proto.z;
console.log("a" in xs[2], "a" in c);
