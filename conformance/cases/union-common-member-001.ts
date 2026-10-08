// A member every member of a union has may be read without narrowing
// (TS §4.13): `.length` on `string | any[]`, a method on a union of
// classes sharing it, and the Object.prototype methods.

function len(v: string | any[]): number {
  return v.length;
}
console.log(len("abc"), len([1, 2]));

function arrs(v: number[] | string[]): number {
  return v.length;
}
console.log(arrs([1, 2, 3]), arrs(["a"]));

function slice2(v: string | number[]): string {
  return String(v.slice(0, 2));
}
console.log(slice2("hello"), slice2([7, 8, 9]));

interface Named {
  name(): string;
}
class P implements Named {
  name(): string {
    return "P";
  }
}
class Q implements Named {
  name(): string {
    return "Q";
  }
}
function nm(x: P | Q): string {
  return x.name();
}
console.log(nm(new P()), nm(new Q()));

function str(v: string | number | boolean): string {
  return v.toString() + "/" + v.valueOf();
}
console.log(str(1), str("a"), str(true));

function pick(b: boolean): string | number {
  return b ? "yes" : 42;
}
console.log(pick(false).toString(), pick(true).toString());

// a field shared by every object member
type Ev = { k: string; v: number } | { k: string; w: boolean };
function key(e: Ev): string {
  return e.k;
}
console.log(key({ k: "a", v: 1 }), key({ k: "b", w: true }));

// iterating a union of iterables
function iter(v: string | string[]): string {
  let out = "";
  for (const c of v) out += "[" + c + "]";
  return out;
}
console.log(iter("ab"), iter(["x", "y"]));

// Array.isArray narrows to the array member
function joinish(v: string | string[]): string {
  if (Array.isArray(v)) return v.join("+");
  return v;
}
console.log(joinish(["a", "b"]), joinish("c"));
