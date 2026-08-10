// RFC-less codegen blade (rotation 356) — AAPCS64 stack args: the 9th+
// argument of a register class travels through the caller's outgoing
// stack area ([sp, #j*8] at the call moment) instead of tripping the
// old 8-GPR ARG_RET cap. Covers direct fns, closures (env + argc + 9
// user args = 11 GPR-lane params), and string-typed overflow args.
function nine(a: any, b: any, c: any, d: any, e: any, f: any, g: any, h: any, i: any): any {
  return a + b + c + d + e + f + g + h + i;
}
console.log(nine(1, 2, 3, 4, 5, 6, 7, 8, 9));

function eleven(
  a: any, b: any, c: any, d: any, e: any, f: any,
  g: any, h: any, i: any, j: any, k: any
): any {
  return "" + a + b + c + d + e + f + g + h + i + j + k;
}
console.log(eleven(1, 2, 3, 4, 5, 6, 7, 8, 9, "x", "y"));

const cl = (a: any, b: any, c: any, d: any, e: any, f: any, g: any, h: any, i: any) =>
  a * 1 + b * 2 + c * 3 + d + e + f + g + h + i;
console.log(cl(1, 2, 3, 4, 5, 6, 7, 8, 9));

// overflow arg is itself a call result (materialize-then-store path)
function tail9(a: any, b: any, c: any, d: any, e: any, f: any, g: any, h: any, i: any): any {
  return i;
}
console.log(tail9(0, 0, 0, 0, 0, 0, 0, 0, nine(1, 1, 1, 1, 1, 1, 1, 1, 1)));
