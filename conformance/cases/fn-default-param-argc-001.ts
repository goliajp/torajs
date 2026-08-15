// A call the default-arg pass padded with the callee's own declared
// defaults must not report those slots as arguments the program
// passed: the head-less tier fills its hidden argc slot from the arg
// list, which that pass rewrites (§10.2.11 binds defaults for the
// missing tail without adding to `arguments`).
function a1(x?: number): number {
  return arguments.length;
}
console.log(a1(), a1(1), a1(1, 2));

function a2(x: number = 5): number {
  return arguments.length;
}
console.log(a2(), a2(1), a2(1, 2));

function a3(x: number): number {
  return arguments.length;
}
console.log(a3(1), a3(1, 2));

function a4(x: number = 1, y: number = 2): number {
  return arguments.length;
}
console.log(a4(), a4(9), a4(9, 8), a4(9, 8, 7));

// §10.2.11 step 26 — an explicit `undefined` binds the default but
// still counts as a passed argument.
console.log(a2(undefined), a4(undefined, undefined));
