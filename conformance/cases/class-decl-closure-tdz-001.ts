// a class declaration's binding is created with the scope and stays
// dead until the declaration evaluates (§8.1.1.1 step 4, ClassDecl
// alongside let / const): a closure written earlier, or a hoisted
// named function a call ahead of the declaration reaches, that reads
// the class name raises ReferenceError; after the declaration both
// answer the class
const read = () => typeof K;
try {
  console.log(read());
} catch (e) {
  console.log(e instanceof ReferenceError, (e as Error).message);
}
function named() {
  return typeof K;
}
try {
  console.log(named());
} catch (e) {
  console.log("named", e instanceof ReferenceError);
}
class K {
  static n = 1;
}
console.log(read(), named(), K.n);
// a closure written after the declaration is not in the dead zone
class L {}
const readL = () => typeof L;
console.log(readL());
// the same shape inside a function body
function outer() {
  const r = () => typeof M;
  try {
    r();
  } catch (e) {
    console.log("inner", e instanceof ReferenceError);
  }
  class M {}
  return r();
}
console.log(outer());
