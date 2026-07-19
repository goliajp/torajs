// torajs intentionally diverges from bun here: an Ident-bound
// fixed-layout (struct-typed) receiver has no __proto__ slot, so
// Object.setPrototypeOf throws a TypeError instead of silently
// no-oping (the silent path read as success while getPrototypeOf
// stayed unlinked — silent-wrong). bun re-parents; the .expected
// file locks the loud boundary until variable-position
// any-promotion lands.
const base: any = { greet: () => "hi" };
const child = { own: 2 };
try {
  Object.setPrototypeOf(child, base);
  console.log("no throw");
} catch (e: any) {
  console.log("caught:", e instanceof TypeError);
}
console.log("still alive:", child.own);
