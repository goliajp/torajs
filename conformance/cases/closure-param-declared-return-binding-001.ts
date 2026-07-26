// An un-annotated binding whose initializer is a call to a function
// that declares what it returns has that type. Without it a class
// instance had no type at all — `new C()` is a call to the synthesized
// factory by the time this runs — so a method call on one of its
// fields had no receiver, and an arrow stored through one kept the
// parameter it gets with no context:
//
//     class C { fs: ((a: number) => number)[] = [] }
//     const c = new C();
//     c.fs.push((a) => a + 1);
//     c.fs[0](3)   // -562949953421311

class Store {
  fs: ((a: number) => number)[] = [];
  xs: number[] = [3, 1, 2];
}

const st = new Store();
st.fs.push((a) => a + 1);
console.log("instance-field-push", st.fs[0](3));

const st2 = new Store();
console.log("instance-field-map", st2.xs.map((x) => x * 2)[0]);
console.log("instance-field-sort", st2.xs.sort((a, b) => a - b)[0]);
console.log("instance-field-filter", st2.xs.filter((x) => x > 1).length);

// Passed on to a function, the annotated parameter was already enough;
// it has to keep working.
function fill(s: Store): void {
  s.fs.push((a) => a * 10);
}
const st3 = new Store();
fill(st3);
console.log("via-annotated-param", st3.fs[0](3));

// Any declared return type, not just a factory's.
function makeOps(): ((n: number) => number)[] {
  return [];
}
const ops = makeOps();
ops.push((n) => n + 1);
console.log("declared-return-array", ops[0](3));

function makeStore(): { fs: ((n: number) => number)[] } {
  return { fs: [] };
}
const ms = makeStore();
ms.fs.push((n) => n + 2);
console.log("declared-return-object", ms.fs[0](3));

function nums(): number[] {
  return [3, 1, 2];
}
const ns = nums();
console.log("declared-return-numbers", ns.map((x) => x * 2)[0]);

type Op = (n: number) => number;
function makeAliased(): Op[] {
  return [];
}
const al = makeAliased();
al.push((n) => n + 3);
console.log("declared-return-alias", al[0](3));

// A field of one of the types the class-field seed learned recently,
// reached through the same binding.
class Stamped {
  d: Date = new Date(21);
}
console.log("instance-date-field", new Stamped().d.getTime());

// A function with no declared return type stays uninferred — a sniff
// is a guess, and this table is read as if it were an annotation.
function guess() {
  return [3, 1, 2];
}
const g = guess();
console.log("undeclared-return", g.length, g[0]);

// Shapes the table already inferred, unchanged.
const lit = [1, 2, 3];
console.log("array-literal-init", lit.map((x) => x + 1)[0]);
const m = new Map<string, number>();
m.set("a", 1);
m.forEach((v, k) => console.log("map-init", k, v));
const n = 5;
const s = "x";
console.log("scalar-inits", n, s);
