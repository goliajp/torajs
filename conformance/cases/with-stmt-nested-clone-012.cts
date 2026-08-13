// §14.11 — a guard's two arms are clones of the same source, and the
// clone has to remember what the original was.
//
// Every marker the compiler puts on an expression is keyed by node id,
// which a clone does not share. The inner `with` here clones the
// right-hand side into its object arm, and that arm holds the
// `String(v)` the parser wrote for `${v}`. Left unmarked, the copy is
// just a read of the name `String` — and the OUTER object, which
// carries one, answers it.
//
// It takes two nested `with`s to see: the inner one makes the clone,
// the outer one is what finds it. The write has to land on the inner
// object for the cloned arm to be the one that runs, so `o1` carries
// `out`.
//
// `.cts` because `with` only exists under the sloppy goal.

var v: any = 5;
var out: any = "untouched";

var o1: any = { out: "carried" };
var o2: any = {
  String: function (x: any): any {
    return "HIJACKED";
  },
};

with (o2) {
  with (o1) {
    out = `v=${v}`;
  }
}

// The inner object took the write, and the substitution stringified
// through the abstract operation rather than through o2.
console.log(o1.out);
// The outer binding never saw it.
console.log(out);
