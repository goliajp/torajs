// A labeled for-of / for-in whose head runs something once before the
// loop — a `var` declaration, a for-in object, a non-identifier source.
// The parser puts that in front of the loop, and the label ended up on
// the group instead of the loop: `continue label` was refused as a
// SyntaxError ("does not label a loop") in a program that is legal.
function f(): string {
  const out: string[] = [];
  a: for (var x of [1, 2, 3]) {
    if (x === 2) continue a;
    out.push("v" + x);
  }
  b: for (const p of ("q,r,s" + "").split(",")) {
    if (p === "r") continue b;
    out.push(p);
  }
  c: for (const k in { m: 1, n: 2 }) {
    for (const j of [1, 2]) {
      if (j === 2) continue c;
      out.push(k + j);
    }
  }
  d: for (const k in { z: 1, y: 2 }) {
    if (k === "y") break d;
    out.push(k);
  }
  return out.join(",") + "/" + x;
}
console.log(f());
