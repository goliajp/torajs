// `new <class static method>()` — §7.2.4 IsConstructor: a method has
// no [[Construct]], so the runtime-construct kernel answers the spec
// TypeError (Error/isError/is-a-constructor.js). r293: the NewDynamic
// CALLEE (desugared to the bare `__sm_Error__isError` ident) rides
// the wrapped closure lane instead of panicking at box_to_any.
try {
  new (Error.isError as any)();
  console.log("constructed");
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}

// the alias form answered the TypeError before r293 — keep both faces
const alias: any = Error.isError;
try {
  new alias();
  console.log("constructed-alias");
} catch (e: any) {
  console.log("caught-alias", e instanceof TypeError);
}

// the value itself still works as a call
console.log(Error.isError(new Error("x")), Error.isError({}));
