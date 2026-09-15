// A declared class field is a plain data property: a value the typed
// slot cannot hold still lands (RFC 20260915-declared-field-shadow).
// The any lane keeps it in the field's sidecar; own faces read it
// before the slot; a value the slot can hold supersedes it.
const fn = function () {};
class C { a; b = 42; c = fn; 'd' = 1; 0 = 2; }
const c: any = new C();
c.b = "unlikelyValue";
console.log(c.b, typeof c.b, c["b"], Object.getOwnPropertyDescriptor(c, "b").value);
console.log(Object.keys(c).join(","), Object.values(c).length, JSON.stringify(c));
console.log("b" in c, c.hasOwnProperty("b"), Object.entries(c)[2].join("="));
c.b = undefined;
console.log(c.b, typeof c.b, Object.getOwnPropertyDescriptor(c, "b").value);
c.b = 7;
console.log(c.b, typeof c.b);
c.b = { x: 1 };
console.log(c.b.x, JSON.stringify(c.b));
delete c.b;
console.log(c.b, "b" in c, Object.keys(c).join(","));
c.b = "back";
console.log(c.b, "b" in c, Object.keys(c).length);
c.d = null; c[0] = "zero";
console.log(c.d, c[0], Object.keys(c).length);
Object.defineProperty(c, "d", { value: "def" });
console.log(c.d, Object.getOwnPropertyDescriptor(c, "d").writable);
Object.defineProperty(c, "d", { value: 5, writable: false });
console.log(c.d, Object.getOwnPropertyDescriptor(c, "d").writable);
console.log(Reflect.set(c, "d", "no"), c.d, Reflect.set(c, "d", 9), c.d);
Object.freeze(c);
console.log(Reflect.set(c, "b", 1), c.b);
