// r505 — a fn-as-object member read walks the §10.4 [[Prototype]]
// chain a `setPrototypeOf` installed, on a named fn and on an arrow,
// for a data property, a method, an accessor, a shadowing own
// expando, a null [[Prototype]], and the inherited
// `Function.prototype` expando. (The inline own-expando probe this
// replaces answered undefined for everything inherited.)
function f(): number { return 1; }
const proto: any = {
  hello: "hi",
  n: 7,
  greet(): string { return "greet:" + this.n; },
  get twice(): number { return this.n * 2; },
};
Object.setPrototypeOf(f, proto);
console.log(Object.getPrototypeOf(f) === proto);
console.log((f as any).hello, (f as any).n, (f as any).missing, (f as any).twice);
console.log((f as any).greet(), f());
(f as any).n = 100;
console.log((f as any).n, proto.n, (f as any).twice);
const g = (x: number): number => x + 1;
Object.setPrototypeOf(g, proto);
console.log((g as any).hello, (g as any).greet(), g(1), typeof (g as any).call);
Object.setPrototypeOf(g, null);
console.log((g as any).hello, typeof (g as any).call, g(2));
(Function.prototype as any).zz = "fp";
function h(): number { return 3; }
console.log((h as any).zz, (f as any).zz, (g as any).zz);
const o: any = { a: 1 };
Object.setPrototypeOf(o, proto);
console.log(o.hello, o.a, o.greet());
