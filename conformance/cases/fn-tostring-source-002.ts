// RFC 20260719-fn-tostring-source B4b — static-tier f.toString()
// on a top-level fn ident folds the type-erased source at compile
// time; byte-identical to the any-lane registry answer.
function add(a: number, b: number): number {
  return a + b;
}
function greet(name: string): string {
  return "hi " + name;
}
console.log(add.toString());
console.log(greet.toString());
const viaAny: any = add;
console.log(add.toString() === viaAny.toString());
console.log(add(1, 2), greet("x"));
