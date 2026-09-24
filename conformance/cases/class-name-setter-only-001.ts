// A class whose own name property is a static accessor with no getter
// reads it as undefined (§10.1.8.1: the accessor's [[Get]] is undefined).
class D { static set name(_: any) {} }
console.log(D.name, typeof D.name);
class G { static get name() { return "gg"; } }
console.log(G.name);
class H {}
console.log(H.name);
const o: any = Object.create(null);
Object.defineProperty(o, "name", { set(v: any) {}, configurable: true });
console.log(o.name);
class L { static set length(_: any) {} }
console.log(L.length);
