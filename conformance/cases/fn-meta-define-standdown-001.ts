// `length` and `name` fold at compile time off a bare declaration
// base, which is right until something redefines them. §20.2.3 makes
// both CONFIGURABLE, so a define is legal — and it landed on the
// declaration's cell while the fold kept answering the declaration,
// so one function told two stories depending on which spelling
// reached it.

function g0(): void {}
Object.defineProperty(g0, "name", { value: "defined" });
console.log(g0.name, (g0 as any).name);

function i0(a: number): void {}
Object.defineProperty(i0, "length", { value: 5 });
console.log(i0.length, (i0 as any).length);

function j0(a: number): void {}
Object.defineProperty(j0, "name", { get: function () { return "GG"; } });
console.log(j0.name, (j0 as any).name);

function k0(a: number, b: number): void {}
Object.defineProperties(k0, { name: { value: "multi" }, length: { value: 3 } });
console.log(k0.name, k0.length);

function l0(a: number): void {}
Reflect.defineProperty(l0, "name", { value: "reflected" });
console.log(l0.name);

// the declared arity still wins where nothing redefined it — a
// default-valued parameter is not counted (§20.2.4.1), and the
// stand-down must not swap in the shell's own arity
function m0(a: number, b: number = 39): void {}
Object.defineProperty(m0, "zz", { value: 1 });
console.log(m0.length, m0.name);

// a function nothing reflects on keeps the fold
function n0(a: number, b: number): void {}
console.log(n0.length, n0.name);
