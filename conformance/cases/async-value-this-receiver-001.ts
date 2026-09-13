// An async function VALUE reading `this` moves too: the generator
// takes the receiver as its leading parameter and the wrapper hands
// its own `this` in — an arrow's lexical `this` (the enclosing
// function's `.call` receiver, or the module's own object at the top
// level), a function expression's call-site receiver — and the body
// suspends at its first await (`b` between `a` and `c`).
function g(this: any, k: number): Promise<any> {
  const base = k * 2;
  const h = async () => { console.log("a"); await null; console.log("c"); return this.k + base; };
  return h();
}
g.call({ k: 3 }, 5).then((x: any) => console.log("arrow-in-fn", x));
const top = async () => { await null; return this; };
top().then((v: any) => console.log("arrow-top", typeof v, v === this));
const fe = async function () { console.log("a"); await null; console.log("c"); return (this as any).q; };
fe.call({ q: 5 }).then((x: any) => console.log("fnexpr", x));
async function outer(): Promise<any> {
  const inner = async () => { await null; return (this as any).w * 2; };
  await null;
  return (await inner()) + (this as any).w;
}
outer.call({ w: 7 }).then((x: any) => console.log("arrow-in-async-fn", x));
console.log("b");
