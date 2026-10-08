// `T | null | undefined` with the nullish members after T, before it
// and around it: all three spell the same type.

class Node1 {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

function val(n: Node1 | null | undefined): string {
  if (n === null) return "null";
  if (n === undefined) return "undef";
  return "v" + n.v;
}
console.log(val(new Node1(3)), val(null), val(undefined));

function lead(v: undefined | null | string): string {
  return v == null ? "nil" : v;
}
function trail(v: string | undefined | null): string {
  return v == null ? "nil" : v.toUpperCase();
}
function around(v: null | string | undefined): string {
  return v ?? "dflt";
}
console.log(lead(null), lead(undefined), lead("t"));
console.log(trail("u"), trail(null), trail(undefined));
console.log(around(null), around("m"), around(undefined));

let s: string | null | undefined = undefined;
console.log(s === undefined, s);
s = "k";
console.log(s);
s = null;
console.log(s);

// a nullable that may already be null joins to itself
function maybe(h: Node1 | null | undefined): Node1 | null {
  return h === undefined ? null : h;
}
console.log(maybe(undefined), maybe(null), maybe(new Node1(5))?.v);

// with more than one other member
function wide(v: string | number | null | undefined): string {
  if (v === null || v === undefined) return "none";
  return "got " + v;
}
console.log(wide(null), wide(undefined), wide(0), wide("z"));

// a literal member followed by null
type L = "a" | "b" | null;
const l1: L = "a";
const l2: L = null;
console.log(l1, l2);
