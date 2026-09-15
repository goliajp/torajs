// §6.2.5.5 GetValue — ToObject(base) before the deferred ToPropertyKey:
// on a nullish base the key expression runs, its toString does not,
// the rhs does not, and the error is a TypeError.
let evaluated: string[] = [];
function key(): any {
  evaluated.push("key");
  return {
    toString() {
      evaluated.push("toString");
      return "k";
    },
  };
}
function rhs(): number {
  evaluated.push("rhs");
  return 1;
}
const base: any = null;
try {
  base[key()] *= rhs();
} catch (e) {
  console.log(e instanceof TypeError);
}
console.log(evaluated.join(" "));
const u: any = undefined;
evaluated = [];
try {
  u[key()] += rhs();
} catch (e) {
  console.log(e instanceof TypeError);
}
console.log(evaluated.join(" "));
evaluated = [];
const ok: any = { k: 5 };
ok[key()] *= rhs();
console.log(ok.k, evaluated.join(" "));
evaluated = [];
try {
  base[key()] ??= rhs();
} catch (e) {
  console.log(e instanceof TypeError);
}
console.log(evaluated.join(" "));
evaluated = [];
try {
  u[key()] ||= rhs();
} catch (e) {
  console.log(e instanceof TypeError);
}
console.log(evaluated.join(" "));
