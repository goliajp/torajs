// The first `F.prototype = …` on a fresh function replaces the virtual
// pair in ONE bag: attributes stay {writable, !enumerable,
// !configurable}, the old default object is let go, the new one serves.
function mk(): any { const F: any = function () {}; F.prototype = { own: 1, hi() { return "hi" } }; return F }
let n = 0;
for (let i = 0; i < 300; i++) { const F = mk(); const o: any = new F(); n += o.own + (o.hi() === "hi" ? 1 : 0) + (o instanceof F ? 1 : 0); }
console.log(n);
const G: any = function () {};
const before = G.prototype;
G.prototype = { tag: "new" };
const d = Object.getOwnPropertyDescriptor(G, "prototype")!;
console.log(d.writable, d.enumerable, d.configurable, G.prototype.tag, G.prototype !== before, before.constructor === G);
console.log(Object.keys(G).join("|"), Reflect.deleteProperty(G, "prototype"), new G().tag, new G() instanceof G);
G.prototype = before;
console.log(new G().constructor === G, Object.getPrototypeOf(new G()) === before);
