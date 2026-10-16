// `(o?.m)(args)` with a nullish `o` evaluates heap-allocated
// arguments (string concatenation, array and object literals) and
// drops them before throwing; a long loop keeps memory flat.

function label(i: number): string {
  return "item-" + i + "-" + "x".repeat(i % 7);
}
function call(o: any, i: number): number {
  try {
    return (o?.m)(label(i) + "/" + i, [i, label(i)], { k: label(i) });
  } catch (e) {
    return e instanceof TypeError ? -1 : -2;
  }
}
const n = process.argv.length > 99 ? 0 : 1;
const present = {
  m(s: string, a: any[], b: any) {
    return s.length + a.length + b.k.length;
  },
};
console.log(call(present, 3));
let caught = 0;
for (let i = 0; i < 200000; i++) {
  if (call(n > 0 ? null : present, i) === -1) caught++;
}
console.log("caught", caught);
