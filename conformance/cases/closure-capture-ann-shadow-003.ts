// objlit face of 001/002: a method-carrying literal's data-field
// idents must resolve at the construction site too — `px: x` under
// `x: number` used to take a later fn's `x: any` ann, laying the
// `__ObjLit_n` slot out as any while the fill wrote raw number bits
// (this.px read garbage / SIGSEGV). Also locks the dead-arena guard:
// the arguments-rewrite pass re-adds the literal and the stale copy
// must not mint a second (first-writer-wins) layout.
function mkPoint(x: number, y: number) {
  return {
    px: x,
    py: y,
    sum() {
      return this.px + this.py;
    },
  };
}
const o = mkPoint(3, 4);
console.log(o.px, o.py, o.sum());
function isErr(x: any): boolean {
  return x === 42;
}
console.log(isErr(42));
