// `switch (u.kind)` over a discriminated union (TS §4.24): each case
// sees the members its literal selects, a case reached by falling
// through also sees the one before it, and the default sees the
// members no case selects.

type Num = { tag: "num"; value: number };
type Add = { tag: "add"; left: number; right: number };
type Neg = { tag: "neg"; operand: number };
type Text = { tag: "text"; body: string };
type Node = Num | Add | Neg | Text;

function evaluate(n: Node): number {
  switch (n.tag) {
    case "num":
      return n.value;
    case "add":
      return n.left + n.right;
    case "neg":
      return -n.operand;
    default:
      return n.body.length;
  }
}

function show(n: Node): string {
  let out = "";
  switch (n.tag) {
    case "add":
      out = "(" + n.left + " + " + n.right + ")";
      break;
    case "num":
    case "neg":
      out = "unary:" + n.tag;
      break;
    case "text":
      out = "'" + n.body + "'";
  }
  return out;
}

const nodes: Node[] = [
  { tag: "num", value: 7 },
  { tag: "add", left: 2, right: 3 },
  { tag: "neg", operand: 4 },
  { tag: "text", body: "four" },
];
let total = 0;
for (const n of nodes) {
  const v = evaluate(n);
  total += v;
  console.log(n.tag, v, show(n));
}
console.log("total", total);
console.log(evaluate({ tag: "add", left: 2, right: 3.5 }), show({ tag: "neg", operand: 1 }));
