// Discriminated unions (TS §4.24): `if (u.kind === "...")` narrows the
// binding to the members whose `kind` is that literal, so a field only
// those members have is readable in the branch — also in the `else`,
// after an early return, through `!==`, a ternary, and a `const`
// holding the literal.

type Circle = { kind: "circle"; r: number };
type Rect = { kind: "rect"; w: number; h: number };
type Label = { kind: "label"; text: string };
type Shape = Circle | Rect | Label;

const RECT = "rect";

function area(s: Shape): number {
  if (s.kind === "circle") {
    return Math.round(s.r * s.r * 3.14159 * 100) / 100;
  } else if (s.kind === RECT) {
    return s.w * s.h;
  }
  return s.text.length;
}

function describe(s: Shape): string {
  if (s.kind !== "label") {
    return s.kind === "circle" ? "circle r=" + s.r : "rect " + s.w + "x" + s.h;
  }
  return "label " + s.text;
}

function width(s: Shape): number {
  if (s.kind !== "rect") return -1;
  return s.w;
}

const shapes: Shape[] = [
  { kind: "circle", r: 2 },
  { kind: "rect", w: 3, h: 4 },
  { kind: "label", text: "hello" },
];
for (const s of shapes) {
  console.log(s.kind, area(s), describe(s), width(s));
}

let cur: Shape = { kind: "rect", w: 1.5, h: 2 };
if (cur.kind === "rect") {
  console.log("rect area", cur.w * cur.h);
}
cur = { kind: "label", text: "x" };
console.log(cur.kind);

interface Ok {
  status: "ok";
  value: number;
}
interface Err {
  status: "err";
  message: string;
}
type Result = Ok | Err;

function unwrap(r: Result): string {
  return r.status === "ok" ? "value " + r.value : "error " + r.message;
}
console.log(unwrap({ status: "ok", value: 4 }), unwrap({ status: "err", message: "bad" }));
