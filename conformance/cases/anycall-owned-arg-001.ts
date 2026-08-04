// An owned temp handed straight to an any-call gets released after it
// — and not before.
//
// `pack_any_argv` passes an argument that is already an `Any` into the
// argv block verbatim, since the runtime only borrows argv and there
// is nothing to box. Nothing released it either, and some of those
// operands are OWNED: an any-member read mints its result, and so
// does an inner any-call.
//
// `new Promise(executor)` is what made it visible. Its desugar calls
// the executor as `__ex(__pr.resolve, __pr.reject)`, so every mint
// stranded both settle closures — and each closure holds an env that
// holds the promise cell, which is why the loss was ~885 bytes per
// `new Promise` rather than the size of a pointer. Binding the same
// read to a `const` first never leaked, which is what a missing temp
// release looks like rather than a wrong refcount.
//
// A release is only correct if the value is still alive for the call
// AND for anyone else holding it, so every case here reads the value
// again afterwards. The output would not survive an over-release; it
// is the same reason a leak fix needs a correctness fixture and not
// only a smaller number.

const holder: any = {
  s: "held",
  f: (v: any) => {
    return "f(" + v + ")";
  },
};

// A — a Str member read as a direct any-call argument, then read
// again from the object it came from.
const callStr: any = (v: any) => {
  return "got:" + v;
};
console.log("A", callStr(holder.s), holder.s, holder.s.length);

// B — a closure member read as a direct argument, then invoked from
// the object afterwards.
const callFn: any = (g: any) => {
  return g(1);
};
console.log("B", callFn(holder.f), holder.f(2));

// C — the shape the leak was found through: an executor gets both
// settle closures as direct any-call arguments, and both still work.
new Promise<string>((res, rej) => {
  res("resolved");
}).then((v) => {
  console.log("C", v);
});

new Promise<string>((res, rej) => {
  rej("rejected");
}).then(
  (v) => {
    console.log("D-unreachable", v);
  },
  (e: any) => {
    console.log("D", e);
  }
);

// E — an inner any-call's result as the outer's argument: owned on
// the way in, and the outer's own answer still readable.
const wrap: any = (v: any) => {
  return "[" + v + "]";
};
console.log("E", wrap(wrap(holder.s)));

// F — the same object survives all of it.
console.log("F", holder.s, holder.f(3));
