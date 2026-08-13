// An escape-boxed binding read into an object literal field owes its
// own share. `padding` is captured by the closure, so it lives in a
// capture box; reading the ident answers the BOX's payload as a
// borrow while the frame exit still releases the whole box. The
// field store went bare (the capture-box preamble registers the
// binding `moved: true, borrowed: false`, which read as "already
// transferred"), so the discarded literal's drop walk released a
// stake nobody took and the symbol was freed under the live array.
const sym: any = Symbol("s");
const xs: any[] = [sym];

for (var padding of xs) {
  var f = function (): any {
    return { padding };
  };
  // a DISCARDED literal — its drop is what over-released
  ({ padding });
}

// the array's element must still be the same live symbol
console.log(String(xs[0]));
console.log(xs[0] === sym);

// and so must the box's payload, read back through the closure
console.log(typeof f().padding);
console.log(String(f().padding));
console.log(f().padding === sym);

// the same shape with a heap string, which never regressed — it
// pins that the share is added, not that the drop was removed
const s2: any = "abcdefghijklmnopqrstuvwxyz0123456789";
const ys: any[] = [s2];
for (var q of ys) {
  var g = function (): any {
    return { q };
  };
  ({ q });
}
console.log(ys[0], g().q === s2);
