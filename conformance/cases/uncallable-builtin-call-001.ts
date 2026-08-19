// Calls to uncallable builtins resolve to their runtime TypeError
// (§13.3.6.2 step 6) instead of a compile-time rejection — namespace
// objects with no [[Call]] and new-only constructors alike. Argument
// side effects still happen first (step 4 sequencing).
const log: string[] = [];
function side(n: string): string {
  log.push(n);
  return n;
}
try {
  // @ts-ignore
  JSON();
} catch (e) {
  console.log("json", e instanceof TypeError);
}
try {
  // @ts-ignore
  Math(side("a"), side("b"));
} catch (e) {
  console.log("math", e instanceof TypeError);
}
console.log("order", log.join(","));
try {
  // @ts-ignore
  Map();
} catch (e) {
  console.log("map", e instanceof TypeError);
}
try {
  // @ts-ignore
  Promise([]);
} catch (e) {
  console.log("promise", e instanceof TypeError);
}
try {
  // @ts-ignore
  Reflect(1);
} catch (e) {
  console.log("reflect", e instanceof TypeError);
}
