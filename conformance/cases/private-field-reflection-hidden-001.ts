class C { #x = 1; y = 2; get x() { return this.#x; } }
const c = new C();
console.log(Object.keys(c), JSON.stringify(c));
const a: any = c;
console.log(Object.keys(a), Object.entries(a), { ...a });
console.log(c);
for (const k in c) console.log("k", k);
console.log(Object.getOwnPropertyNames(a), Reflect.ownKeys(a));
class D { #z = 7; w = 1; static #s = 2; #m() { return this.#z; } run() { return this.#m() + D.#s; } }
const d = new D();
console.log(JSON.stringify(d), Object.values(d), Object.entries(d), Object.assign({}, d), d.run());
class E { #only = 1; peek() { return this.#only; } }
console.log(new E(), Object.keys(new E()), JSON.stringify(new E()), new E().peek());
const { ...rest } = d;
console.log(rest);
