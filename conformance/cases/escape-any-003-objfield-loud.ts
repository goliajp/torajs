// L3b #2 / RFC 20260706-typed-arr-any-escape B3 — edge 4 loud backstop:
// an object-literal `any` FIELD store is not an escape seed in v1
// (field annotations grow no seed; RFC 记档面). The array stays typed;
// the kind-changing write through the any field hits the mark_kind
// dynamic backstop and throws a loud TypeError — never silent.
//
// bun accepts the write and transmutes ([w, 2]); the .expected file
// locks tr's loud-not-silent contract until the obj-any-field seed
// lane ships. If this case ever prints "wrote:" the backstop went
// silent — that is a P0 regression, not a fixture to update.
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
