// §14.11 — an object literal used as a `with` scope object widens to
// `: any` at its declaration (rotation 437): the scope object is
// dynamic by contract (per-reference HasBinding, body-side `delete`,
// getter re-entrance, fall-through misses), which a nominal struct
// stamp cannot express.

// getter deletes its own backing slot mid-read (the S11.13.2 shape)
var scope = {
  get x() {
    delete this.x;
    return 2;
  }
};
var x = 0;
var r: any = 0;
with (scope) {
  x ^= 3;
}
console.log(r = scope.x, x); // 1 0 — PutValue used the initial ref

// nested `with`: the inner head is itself guard-wrapped by the outer
// desugar; the widen peels to the fall-through binding
var outer = { y: 0 };
var y: any = "outerlex";
var inner = {
  get y() { delete this.y; return 2; }
};
var t: any = 0;
with (outer) {
  with (inner) {
    t = y;
  }
}
console.log(t, inner.y, outer.y); // 2 undefined 0

// member miss on the widened object answers undefined
console.log(scope.nothere);
