// A typed array stored into an object-literal field annotated `any`
// reaches the any world through that field, so a kind-changing write
// through it lands and typed reads of the array see it (bun: [w, 2]).
const t: number[] = [1, 2];
const o: { f: any } = { f: t };
try {
  o.f[0] = "w";
  console.log("wrote:", t[0]);
} catch (e) {
  console.log("caught");
}
console.log("after:", t[0], t.length);
// same-kind write through the any field is legal and lands
o.f[1] = 42;
console.log("samekind:", t[1]);
