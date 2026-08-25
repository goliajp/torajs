// The narrow edge of the per-static table: Symbol.for is a MINTING
// static — with no __torajs_symbol_ construction symbol in the
// program, the symbol arm stays alive only through the table's
// FAM_SYMBOL bit. The template coercion must reach the real spec
// TypeError (message and all), not a stub reject.
const sfor = Symbol.for;
const sym = sfor("k");
try {
  console.log(`x${sym}`);
} catch (e) {
  console.log("caught", (e as any).message);
}
