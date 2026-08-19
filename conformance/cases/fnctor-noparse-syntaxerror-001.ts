// §20.2.1.1 steps 11/17 — assembled dynamic-function text that fails
// to parse answers a creation-time SyntaxError (thrown when the call
// runs), never a compile error. Same posture the eval channel already
// takes for §19.2.1.1 step 12.
function probe(f: () => unknown): string {
  try {
    f();
    return "no-throw";
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
// LabelledItem never derives a lexical declaration (§14.13).
console.log(probe(() => Function("a: let x;")));
console.log(probe(() => Function("b: const y = 3;")));
console.log(probe(() => Function("c: class z {};")));
// §B.3.2 admits a labelled function in sloppy code only.
console.log(probe(() => Function("'use strict'; d: function w() {};")));
// for-head NoIn / no-initializer restrictions (§14.7.4 / §14.7.5).
console.log(probe(() => Function("for (var x = 3 in {}; ; ) break;")));
console.log(probe(() => Function("for (var x = 3 of 42);")));
// §B.1.3 — `-->` with no preceding line terminator is not a comment,
// and `--` cannot open a parameter list.
console.log(probe(() => Function("-->", "")));
// import.meta is module-only syntax (§13.3.12); dynamic-function text
// is script code.
console.log(probe(() => Function("import.meta")));
console.log("done");
