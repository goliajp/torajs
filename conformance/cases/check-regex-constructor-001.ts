// ES §22.2.3.1 — `new RegExp(pattern, flags)` constructor. Static
// string-literal args are already desugared to `Expr::Regex { ... }`
// in `desugar_builtin_new`; this fixture covers the dynamic-arg
// shape (variable-bound pattern / flags) which goes through the
// runtime `__torajs_regex_compile` helper at call time.
const pat = "foo";
const flags = "i";
const re = new RegExp(pat, flags);
console.log(re.source);                  // "foo"
console.log(re.flags);                   // "i"
console.log(re.ignoreCase);              // true
console.log(re.test("FOO"));             // true (ignoreCase)
console.log(re.test("bar"));             // false

// 1-arg dynamic: only pattern, default flags
const re2 = new RegExp(pat);
console.log(re2.source);                 // "foo"
console.log(re2.flags);                  // ""
console.log(re2.test("foo"));            // true

// pattern composed at runtime
function makeRe(prefix: string): RegExp {
  return new RegExp(prefix + "[0-9]+", "g");
}
const re3 = makeRe("user");
console.log(re3.source);                 // "user[0-9]+"
console.log(re3.flags);                  // "g"
console.log(re3.test("user42"));         // true
