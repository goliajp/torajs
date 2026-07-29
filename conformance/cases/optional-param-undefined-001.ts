// §9.2 — an absent optional parameter binds undefined, not null:
// `typeof b`, strict-eq against undefined/null, and default-guard
// interplay all observe the real undefined box on an `any` /
// un-annotated optional slot.
function f(a: number, b?: any) {
  console.log(b, typeof b, b === undefined, b === null);
}
f(1);
f(1, 7);
f(1, null);

class C {
  m(x: number, y?: any) {
    console.log("m", y === undefined);
  }
}
new C().m(1);
new C().m(1, 0);
