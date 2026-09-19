// A TS parameter property is declared ahead of the class's own fields,
// wherever the constructor sits in the body, and its assignment runs
// after `super(…)` and after the field initializers. Own-key order,
// JSON.stringify and for-in all read that order; tr used to list the
// declared fields first and, in a derived class, assign the parameter
// properties in front of `super(…)`.
class A { x = 1; constructor(public tag: string) {} }
console.log(Object.keys(new A("t")).join(","));
class B { x = "bx"; y: string; constructor(public tag: string, readonly n: number) { this.y = tag + n; } }
const b = new B("t", 2); console.log(b.x, b.y, b.tag, b.n, Object.keys(b).join(","));
class P { p = "p"; constructor(public base: number) {} }
class D extends P { d = "d"; constructor(public extra: number) { super(extra * 10); } }
const d = new D(3); console.log(d.d, d.extra, d.base, Object.keys(d).join(","));
class E { #h = 1; after = 2; constructor(private secret: string, public shown: string) {} peek(): string { return this.secret + this.#h; } }
const e = new E("s", "v"); console.log(e.peek(), Object.keys(e).join(","), JSON.stringify(e));
class F { first = "f"; constructor(public a: number, public b: number) {} last = "l"; }
const f = new F(1, 2); const seen: string[] = []; for (const k in f) seen.push(k);
console.log(Object.keys(f).join(","), JSON.stringify(f), seen.join(","));
class G extends F { g = "g"; constructor(public own: string) { super(7, 8); } }
console.log(JSON.stringify(new G("o")));
