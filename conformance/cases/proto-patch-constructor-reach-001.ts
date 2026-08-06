// A builtin prototype is reachable without ever naming its
// constructor: `.constructor` hands one back off any value of that
// family. The whole-program scan attributes what it can spell and
// stands everything down when it cannot — what it must never do is
// miss the write and silently ignore the patch.

// 1. spellable + direct — a literal receiver names Array as surely
//    as `Array` does, so this attributes to one family
([].constructor as any).prototype.join = function () {
  return "J";
};
const a: number[] = [1, 2, 3];
console.log("direct  :", String(a.join("-")));

// 2. spellable + escaping — the constructor lands in a variable, the
//    same shape `const A = Array` takes
const C: any = [].constructor;
(C.prototype as any).lastIndexOf = function () {
  return 42;
};
const b: number[] = [1, 2, 3];
console.log("aliased :", String(b.lastIndexOf(2)));

// 3. index form spells the same thing
const D: any = ([] as any)["constructor"];
(D.prototype as any).indexOf = function () {
  return 7;
};
const c: number[] = [1, 2, 3];
console.log("indexed :", String(c.indexOf(2)));

// 4. unspellable — the receiver's family is not syntactic, so every
//    family stands down rather than the write going unseen
const seed: any = "x";
const S: any = seed.constructor;
(S.prototype as any).toUpperCase = function () {
  return "U";
};
const s: string = "abc";
console.log("unspell :", String(s.toUpperCase()));

// 5. a harmless read must still answer, and must not disturb anything
class Foo {}
const foo: any = new Foo();
console.log("readonly:", String(foo.constructor.name), String([9, 8].length));
