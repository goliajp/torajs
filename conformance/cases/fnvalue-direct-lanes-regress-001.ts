// Regression pins for the shapes that must KEEP their existing lanes:
// bare arrow, capturing arrow, and a direct named-fn argument.
function apply(f: () => number): number {
  return f();
}
function h(): number {
  return 42;
}
console.log("arrow", apply(() => 41));
let n = 5;
console.log("capture", apply(() => n + 1));
console.log("direct", apply(h));
let alias = h;
console.log("alias", apply(alias));
