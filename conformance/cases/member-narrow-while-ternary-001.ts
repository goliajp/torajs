// member-path truthiness narrow at while and ternary guard sites
// (chunk-761 archive: collect_member_narrow was if-stmt only). The
// while body narrow is gated off when the body re-binds the
// receiver or re-assigns the member (conservative loud, next-
// iteration safety).
type O = { cb?: () => number; s?: string };

function f(o: O): number {
  let n = 0;
  while (o.cb) {
    n += o.cb();
    if (n > 0) {
      break;
    }
  }
  const m = o.s ? o.s.length : -1;
  return n + m;
}
console.log(f({ cb: () => 5, s: "abc" }));
console.log(f({ cb: undefined, s: undefined }));

// else-polarity ternary (`=== undefined` guards the else branch)
function h(o: O): number {
  return o.s === undefined ? -1 : o.s.length;
}
console.log(h({ cb: undefined, s: "abcd" }));
console.log(h({ cb: undefined, s: undefined }));
