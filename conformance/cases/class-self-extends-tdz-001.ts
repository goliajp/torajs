// ES 15.7.14 - the class name binds (uninitialized) before the
// heritage evaluates, so `class x extends x` throws a ReferenceError
// at the definition, in both statement and expression position.
try {
  class x extends x {}
} catch (e: any) {
  console.log(e.constructor.name);
}
try {
  const C: any = class y extends y {};
  console.log("no-throw", C);
} catch (e: any) {
  console.log(e.constructor.name);
}
console.log("after");
