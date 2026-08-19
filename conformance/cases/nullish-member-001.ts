// Member reads and method calls on a statically-nullish receiver
// throw the §13.2.3 runtime TypeError instead of stopping the
// compile.
try {
  // @ts-ignore
  undefined.toString();
} catch (e) {
  console.log("mc-undef", e instanceof TypeError);
}
try {
  // @ts-ignore
  null.constructor;
} catch (e) {
  console.log("rd-null", e instanceof TypeError);
}
try {
  // @ts-ignore
  undefined.foo;
} catch (e) {
  console.log("rd-undef", e instanceof TypeError);
}
function v(): void {}
try {
  // @ts-ignore
  v().next();
} catch (e) {
  console.log("mc-void", e instanceof TypeError);
}
const u: undefined = undefined;
try {
  // @ts-ignore
  u.description;
} catch (e) {
  console.log("rd-binding", e instanceof TypeError);
}
