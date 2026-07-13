// RFC 20260714-t262-top-clusters 刀 2 — generic Array.prototype
// methods over static-struct and primitive receivers. The reified
// `.call` re-dispatch already routed array-family mids to the
// arraylike arm for Tag::DynObj receivers, but an object LITERAL
// receiver lowers as a Tag::Obj anon struct ("struct receivers stay
// on the plain TypeError" was the recorded backlog), and bool
// receivers had no array arm at all. Now: struct receivers ride the
// same generic arm (length via the class-layouts probe, index reads
// via the chunk-744 struct lanes; mutators stay excluded), and bool
// receivers run the empty-receiver semantics (ToObject(bool) has no
// own length → vacuous loops, IsCallable checks preserved).

// struct receiver — scan family
console.log(Array.prototype.indexOf.call({ 0: true, 1: 1, length: 2 }, true));
console.log(Array.prototype.indexOf.call({ length: null }, 1));
console.log(Array.prototype.lastIndexOf.call({ 0: 5, 1: 5, length: 2 }, 5));
console.log(Array.prototype.includes.call({ 0: "a", length: 1 }, "a"));
console.log(Array.prototype.join.call({ 0: 1, 1: 2, length: 2 }, "-"));

// struct receiver — HOF family
console.log(Array.prototype.every.call({ 0: 2, 1: 4, length: 2 }, (x: any) => x % 2 === 0));
console.log(Array.prototype.some.call({ 0: 1, length: 1 }, (x: any) => x > 0));
console.log(Array.prototype.find.call({ 0: 7, length: 1 }, (x: any) => x === 7));

// bool receiver — vacuous semantics
console.log(Array.prototype.every.call(true, () => {}));
console.log(Array.prototype.every.call(false, () => {}));
console.log(Array.prototype.some.call(true, () => true));
console.log(Array.prototype.indexOf.call(true, 1));
console.log(Array.prototype.includes.call(false, 0));
console.log(Array.prototype.find.call(true, () => true));
console.log(Array.prototype.join.call(true, ","));

// bool receiver — observable spec throws survive the empty loop
try {
  Array.prototype.every.call(true, 42);
} catch (e) {
  console.log("every-noncallable: caught");
}
try {
  Array.prototype.reduce.call(true, (a: any, b: any) => a);
} catch (e) {
  console.log("reduce-empty: caught");
}
console.log(Array.prototype.reduce.call(true, (a: any, b: any) => a, 9));

console.log("done");
