// RFC 20260719-fn-tostring-source B5 — string coercion faces:
// String(f) / template substitution / str+fn concat all answer the
// type-erased source, static tier and any lane alike.
function add(a: number, b: number): number {
  return a + b;
}
console.log(String(add));
console.log(`>>${add}<<`);
console.log("fn: " + add);
console.log(add + "!");
const dbl = (x: number) => x * 2;
console.log(String(dbl));
console.log(`${dbl}`);
const fa: any = add;
console.log(String(fa) === String(add));
console.log(add(2, 3));
