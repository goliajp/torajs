// `s.replaceAll(re, r)` over an any-typed string calls the RegExp's
// @@replace (§22.1.3.20 step 2.c), so a program whose only RegExp
// method call is this one still reaches the RegExp protocol entry; a
// plain global RegExp answers what the replace kernel computes.
const s: any = "a-b-a";
console.log(s.replaceAll(/a/g, "x"));
const re: any = /-/g;
console.log(s.replaceAll(re, "+"));
console.log(s.replaceAll(/(b)/g, "[$1]"));
try {
  s.replaceAll(/a/, "y");
} catch (e: any) {
  console.log("non-global", e instanceof TypeError);
}
