// chunk 708 — any-to-typed BORROW alias (698 owned decode-copy's
// sibling): `const t: number[] = src` with src: any is a JS
// reference alias — top-level K.4 lane admits it (was loud); elem
// reads are kind-aware; mutation through src is visible through t.
const src: any = [10, 20, 30];
const t: number[] = src;
console.log(t[0], t[1], t[2], t.length);
src.push(40);
console.log(t[3], t.length);
let sum = 0;
for (const v of t) sum += v;
console.log(sum);
const s2: any = ["a", "b"];
const ts: string[] = s2;
console.log(ts[0], ts[1]);
function f() {
  const fsrc: any = [1, 2];
  const ft: number[] = fsrc;
  fsrc.push(3);
  console.log(ft[2], ft.length);
}
f();
