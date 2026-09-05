// The root of a property store decides whether the slot honours the
// receiver, and a root whose initializer is a CALL has the same shape
// as one written as a literal — the callee's declared return type says
// so. Spelled with a literal these already kept `this`; spelled with a
// factory they did not, which is a difference no one writing either
// could predict from the source.
function fn(a: any) {
  return "a=" + a + "/" + typeof (this as any);
}

function mkArr(): any[] {
  return [];
}
const z = mkArr();
z[0] = fn;
console.log(z[0](1));

function mkRows(): any[][] {
  return [[]];
}
const rows = mkRows();
rows[0][0] = fn;
console.log(rows[0][0](2));

function mkObj(): any {
  return {};
}
const o = mkObj();
o.m = fn;
console.log(o.m(3));
