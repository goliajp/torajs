// 564-01 — a computed member's name is its runtime key, not the
// compiler's sentinel. tr desugars `[k]() {}` to a body named
// `__ccm_<n>__`, and `.name` read that sentinel straight out of the
// fn-name registry. §10.2.9 SetFunctionName says the name is the
// property key: a String key verbatim (a numeric key already
// arrived as its String spelling), a Symbol key as
// "[<description>]", and the empty String for a description-less
// symbol. The face carries it from the definition point, where the
// key exists.
//
// The inspect face is a SEPARATE question and answers differently:
// a computed member has no name in the SOURCE, so bun prints the
// anonymous `[Function]` form for it even though `.name` is set.
const k = "c1";
const num = 7;
const sD = Symbol("d");
const sNo = Symbol();

class W {
  m() {}
  [k]() { return 1 }
  [num]() { return 2 }
  [sD]() { return 3 }
  [sNo]() { return 4 }
  get [k + "g"]() { return 5 }
  static [k + "s"]() { return 6 }
}
const w: any = new W();

console.log(JSON.stringify(w.m.name), JSON.stringify(w[k].name));
console.log(JSON.stringify(w[7].name), JSON.stringify(w[sD].name), JSON.stringify(w[sNo].name));
console.log(JSON.stringify((W as any)[k + "s"].name));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(W.prototype, k + "g")!.get!.name));

// the inspect face: named in the source vs named at runtime
console.log(w.m, w[k], w[7], w[sD], w[sNo]);
console.log((W as any)[k + "s"]);

// the members still work, and still land in declaration order
console.log(w[k](), w[7](), w[sD](), w[sNo](), (W as any)[k + "s"]());
console.log(Object.getOwnPropertyNames(W.prototype));
