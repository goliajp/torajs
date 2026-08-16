// §22.1.3.19 step 2 hands a searchValue that has `@@replace` — every
// RegExp does — straight to that method, and §22.1.3.{11,12,20} do
// the same for `@@match` / `@@matchAll` / `@@search`. Only a value
// WITHOUT one takes the ToString step.
//
// tr's typed-receiver lane decided that statically, off the argument's
// SSA type, and a RegExp reaches it with nothing but `any` written on
// it: `var re = /b/` is an `any` binding once `desugar_var_hoist` has
// split the declaration from its assignment, and so is any parameter
// annotated `any`. So the lane ToString'd the RegExp and used its own
// source text as the pattern — `"abc".replace(re, "Y")` answered
// "abc", `"abc".match(re)` answered null, `"abc".search(re)` answered
// -1. Silently, which is the outcome the design principles rank
// worst. (`split` was already right: its separator has ridden a
// runtime cell-tag dispatch since rotation 264. These are that same
// dispatch, for the callers that mint or replace instead.)

var re: any = /b/;
var glob: any = /b/g;
var caps: any = /([a-z])(\d)/g;

console.log("replace", "abc".replace(re, "Y"));
console.log("replaceAll", "abcb".replaceAll(glob, "Y"));
console.log("match", JSON.stringify("abc".match(re)));
console.log("search", "abc".search(re));
console.log("matchAll", [..."abcb".matchAll(glob)].length);
console.log("caps", "a1b2".replace(caps, "[$1$2]"));
console.log("split", "a-b".split(re).join("|"));

// a STRING in the same `any` slot must stay a literal search for
// replace (§22.1.3.19 step 3 is a substring scan, not a pattern) even
// though the very same slot coerces to a pattern for search / match
var dot: any = ".";
console.log("literal", "a.c".replace(dot, "X"), "a.c".replaceAll(dot, "X"));
console.log("coerced", "a.c".search(dot), JSON.stringify("a.c".match(dot)));

// §22.2.3.2 step 1 — an undefined pattern is the EMPTY pattern for
// match, and the six characters "undefined" for replace's substring
// scan (§22.1.3.19 step 3's ToString)
var u: any = undefined;
console.log("undef", "abc".replace(u, "X"), JSON.stringify("abc".match(u)));

// a number coerces both ways alike
var num: any = 1;
console.log("number", "a1b".replace(num, "X"), "a1b".search(num));

// §22.1.3.20 step 2.b — replaceAll rejects a non-global RegExp before
// it touches anything else
try {
  console.log("no-throw", "abcb".replaceAll(re, "X"));
} catch (e: any) {
  console.log("replaceAll throws", e instanceof TypeError);
}

// the searchValue's own ToString still runs, and still throws first
var thrower: any = {
  toString: function () {
    throw new TypeError("boom");
  },
};
try {
  console.log("no-throw", "abc".replace(thrower, "X"));
} catch (e: any) {
  console.log("toString throws", e.message);
}

// the statically-typed spellings are untouched
console.log("typed", "abc".replace("b", "Y"), "abc".replace(/b/, "Z"));
