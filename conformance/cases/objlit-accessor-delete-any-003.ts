// defineProperty around a deleted object-literal accessor: an
// attribute-only redefine keeps the accessor, a define after the delete
// creates whatever the descriptor says (§10.1.6.3 step 2, absent
// attributes false), and an integrity level still refuses: a frozen
// instance's accessor is not configurable (strict delete throws), and a
// non-extensible instance cannot get the deleted name back.
const f = { get m() { return 1 }, j: 2 };
const fa: any = f;
Object.defineProperty(fa, "m", { enumerable: false });
console.log(fa.m, Object.keys(fa).join(","), Object.getOwnPropertyDescriptor(fa, "m"));
delete fa.m;
console.log(fa.m, "m" in fa);
Object.defineProperty(fa, "m", { value: undefined, writable: true, enumerable: true, configurable: true });
console.log(fa.m, "m" in fa, Object.keys(fa).join(","), f.m);
Object.defineProperty(fa, "m", { get() { return 42 }, configurable: true });
console.log(fa.m, f.m, Object.keys(fa).join(","));

const z = { get q() { return 3 }, r: 0 };
const za: any = z;
Object.freeze(za);
try {
  delete za.q;
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
console.log(za.q, z.q);
const y = { get q() { return 4 } };
const ya: any = y;
delete ya.q;
Object.preventExtensions(ya);
try {
  ya.q = 1;
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
console.log(ya.q, "q" in ya);
