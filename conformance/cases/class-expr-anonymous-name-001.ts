// An anonymous class expression is named only by §8.4 NamedEvaluation
// (a binding, a property, a field it directly initializes). Anywhere
// else its `.name` is "" — whichever lane the class takes.
//
// `.expected` rather than a live bun diff: bun 1.4.2 names the class in
// `[class {}][0]` after the binding, and answers "" for `inner()`'s
// `const L = class { static t = 2 }`; node v26 answers "" and `L`.
function mk(): any { return class {}; }
function mk2(n: number): any { return class { v(): number { return n; } }; }
function mk3(): any { return class { static s = 1; }; }
const arrowed = (): any => class {};
const inArray: any = [class {}][0];
console.log(JSON.stringify(mk().name), JSON.stringify(mk2(1).name), JSON.stringify(mk3().name));
console.log(JSON.stringify(arrowed().name), JSON.stringify(inArray.name));
function o(): any {
  class H {
    static F = ((): any => { return class {}; })();
    static G = class {};
  }
  return H;
}
const h = o();
console.log(JSON.stringify(h.F.name), h.G.name);
// NamedEvaluation still names them
const K = class {};
const obj = { k: class {} };
function inner(): any {
  const L = class { static t = 2; };
  return L;
}
console.log(K.name, obj.k.name, inner().name);
