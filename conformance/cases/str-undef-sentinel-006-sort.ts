// default-comparator sort puts undefined LAST per §23.1.3.30.2 —
// the check happens before ToString, so the sentinel never sorts
// as the text "undefined" (RFC 20260707 residual).
const m = /a(b)?/.exec("a");
const s = "ab";
if (m !== null) {
  const xs = ["z", m[1], "a"];
  xs.sort();
  console.log(xs.join(","));
  console.log(xs[2] === undefined);
  // multiple undefined slots all sink to the tail
  const ys = ["u", m[1], "b", m[1], "a"];
  ys.sort();
  console.log(ys.join(","));
  console.log(ys[3] === undefined);
  console.log(ys[4] === undefined);
}
console.log(s.length);
// the TEXT "undefined" is a real string and sorts by content
const ts = ["z", "undefined", "a"];
ts.sort();
console.log(ts.join(","));
console.log(ts[1] === undefined);
// numeric default sort keeps the ToString lane
const ns = [10, 2, 1];
ns.sort();
console.log(ns.join(","));
