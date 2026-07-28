// ObjectLit returned under a declared struct type with Any-valued
// fields: the declared layout pins the slots and the Any values
// unbox into the declared faces (pre-fix the anon layout kept an
// Any slot and the caller's declared-width read answered NaN)
function mk(): { value: number; done: boolean } {
  const x: any = 42;
  return { value: x, done: false };
}
const r = mk();
console.log(r.value);
console.log(r.value + 1);
console.log(r.done);

function mks(): { tag: string; n: number } {
  const t: any = "lbl";
  const m: any = 5;
  return { tag: t, n: m };
}
const s = mks();
console.log(s.tag);
console.log(s.n * 2);
