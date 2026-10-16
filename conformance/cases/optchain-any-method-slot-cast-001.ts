// An `any` written into a slot whose object type has a method: `null` /
// `undefined` stay nullish to `?.`, a cell the slot can read crosses,
// and a number, boolean, string, array, function or Map throws a
// TypeError before its method is reached. A class extending Map takes
// only instances of itself or a subclass.
type T = { m: () => number };
class K {
  m() {
    return 7;
  }
}
function cast(v: any): string {
  try {
    const p: T | undefined = v;
    return String(p?.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(cast(undefined), cast(null), cast(new K()), cast(5), cast(0), cast(true), cast("ab"));
console.log(cast("a long string that is surely on the heap"), cast([1, 2]), cast(() => 1), cast(new Map()));
const lit: T = { m: () => 5 };
let l: T | undefined;
function assign(v: any) {
  try {
    l = v;
    return String(l?.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(assign(lit), assign(undefined), assign(3), assign("s"));
function local(v: any) {
  let p: T | undefined = { m: () => 1 };
  try {
    p = v;
    return String(p?.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(local(lit), local(null), local("x"), local([1]));
function strict(v: any) {
  try {
    const p: T = v;
    return String(p.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(strict(new K()), strict(undefined), strict(1));
interface I {
  m(): number;
}
class J implements I {
  m() {
    return 8;
  }
}
class Z {
  y = 1;
}
function iface(v: any) {
  try {
    let p: I | undefined = new J();
    p = v;
    return String(p?.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(iface(new J()), iface(new Z()), iface(undefined), iface(4), iface("s"));
class MP extends Map<string, number> {
  tag() {
    return "mp";
  }
}
class MP2 extends MP {}
function exotic(v: any) {
  try {
    const p: MP | undefined = v;
    return String(p?.tag()) + ":" + String(p?.size);
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(exotic(new MP()), exotic(new MP2()), exotic(undefined));
// a cell of another builtin or a plain Map has no `tag`
console.log(exotic(new Map()), exotic("a long string that is surely on the heap"), exotic([1]));
console.log(exotic(() => 1), exotic(new Set()), exotic({}), exotic(3));
