// `typeof x === "tag"` narrows a union binding (TS §4.24): inside the
// guarded branch only the matching members remain, so each branch may
// call its own member's methods.

function describe(v: string | number): string {
  if (typeof v === "string") {
    return v.toUpperCase();
  }
  return v.toFixed(2);
}
console.log(describe("abc"), describe(3.14159));

function f2(v: string | number): string {
  if (typeof v === "number") return "num:" + v.toFixed(1);
  else return "str:" + v.toLowerCase();
}
console.log(f2(2), f2("ABC"));

function f3(v: number | string | boolean): string {
  if (typeof v !== "string") return "notstr " + v;
  return v.charAt(0);
}
console.log(f3(true), f3(5), f3("zed"));

// early return narrows the rest of the body
function early(v: string | number): number {
  if (typeof v !== "string") return v;
  return v.length;
}
console.log(early(3), early("four"));

// else-if chains narrow member by member
function chain(v: number | string | boolean): string {
  if (typeof v === "number") return "n" + v.toFixed(0);
  else if (typeof v === "string") return "s" + v.length;
  else return "b" + (v ? 1 : 0);
}
console.log(chain(1.6), chain("xy"), chain(true));

// a ternary narrows each arm
function tern(v: string | number): number {
  return typeof v === "string" ? v.length : v * 10;
}
console.log(tern("abc"), tern(4));

// a narrowed value fits a parameter of its member's type
function len(s: string): number {
  return s.length;
}
function outer(v: string | number): number {
  if (typeof v === "string") return len(v);
  return v;
}
console.log(outer("four"), outer(9));

// assigning inside the narrowed branch takes any member again
function norm(v: string | number): string {
  if (typeof v === "number") {
    v = String(v);
  }
  return v.toUpperCase();
}
console.log(norm(12), norm("ab"));

// a function member narrows by "function"
function callOr(f: (() => number) | number): number {
  if (typeof f === "function") return f();
  return f;
}
console.log(callOr(() => 7), callOr(8));

// switch on typeof
function sw(v: string | number | boolean): string {
  switch (typeof v) {
    case "string":
      return "S" + v.length;
    case "number":
      return "N" + v.toFixed(1);
    default:
      return "B" + v;
  }
}
console.log(sw("ab"), sw(2), sw(false));

// typeof inside && guards the right operand
function both(v: string | number): boolean {
  return typeof v === "string" && v.startsWith("a");
}
console.log(both("abc"), both("xbc"), both(1));
