// `x.size` on an any receiver answers an ordinary [[Get]] of "size"
// for every shape except the Map / Set live count: a class or exotic
// subclass method, a struct field or getter, an inherited entry, a
// patched Array.prototype. It used to answer undefined for all of them.
class A extends Array { size() { return 5; } }
const a: any = new A();
class C { size() { return 1; } }
class G { get size() { return 2; } }
class F { size = 3; }
const objs: any[] = [a, new C(), new G(), new F(), { size: 4 }, Object.create({ size: 6 }), [1, 2], "str", 7, new Map([[1, 1]]), new Set([1, 2, 3])];
for (const o of objs) console.log(typeof o.size, typeof o.size === "function" ? o.size() : o.size);
(Array.prototype as any).size = 9;
console.log(([1] as any).size);
