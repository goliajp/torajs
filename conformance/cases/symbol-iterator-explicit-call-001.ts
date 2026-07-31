// explicit @@iterator invocation on TYPED receivers — the symbol-keyed
// index-call lane admits non-any receivers (RFC 20260728 F0b widened),
// and the checker's Index arm admits symbol/string keys on Map/Set.
// typed receivers, explicit @@iterator call
const it1 = [1, 2][Symbol.iterator]();
console.log("a", it1.next().value, it1.next().value, it1.next().done);
const m = new Map([["k", 1]]);
const it2 = m[Symbol.iterator]();
console.log("b", JSON.stringify(it2.next().value));
const s = new Set([7, 8]);
const it3 = s[Symbol.iterator]();
console.log("c", it3.next().value);
const it4 = "hi"[Symbol.iterator]();
console.log("d", it4.next().value, it4.next().value, it4.next().done);
// user object with computed @@iterator, typed struct receiver
const o = {
  [Symbol.iterator]() {
    return [9][Symbol.iterator]();
  }
};
const it5 = o[Symbol.iterator]();
console.log("e", it5.next().value);
// spread still fine after the lane widened
console.log("f", [...[3, 4]]);
// numeric index call on typed receiver keeps its own lane
const fns = [() => 42];
console.log("g", fns[0]());
