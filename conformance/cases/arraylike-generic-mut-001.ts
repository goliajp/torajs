// RFC 20260712-array-generic-receiver chunk 3b-1 — generic mutators
// (pop / push / shift / unshift / reverse) over plain-object
// receivers: per-index Get / Set / DeletePropertyOrThrow + the
// spec's trailing Set(O, "length", …) (observable even on the empty
// fast exits). Reached through the stored-reified-cell own-entry
// re-dispatch; the dynobj resize relocation writes the receiver
// back through recv_slot.
//
// Acceptance: byte-equal with bun.

// pop — drains entries, deletes the popped key, shrinks length
var obj: any = {};
obj.pop = Array.prototype.pop;
obj[0] = "x";
obj[1] = "y";
obj.length = 2;
console.log(obj.pop());
console.log(obj.length, obj[1]);
console.log(obj.pop());
console.log(obj.pop());
console.log(obj.length);

// pop on an empty receiver still Sets length (spec step 3.b)
var e: any = { pop: Array.prototype.pop };
console.log(e.pop(), e.length);

// push — appends + returns the new length (relocation exercised by
// the fresh keys the Sets insert)
var p: any = { length: 0 };
p.push = Array.prototype.push;
console.log(p.push("a", "b", "c"));
console.log(p.length, p[0], p[2]);

// shift — head Get, forward moves with the absent-hole Delete leg
var s: any = { 0: 1, 1: 2, 3: 4, length: 4 };
s.shift = Array.prototype.shift;
console.log(s.shift());
console.log(s.length, s[0], s[1], s[2]);

// unshift — backward moves + front stores
var u: any = { 0: "b", 1: "c", length: 2 };
u.unshift = Array.prototype.unshift;
console.log(u.unshift("a"));
console.log(u.length, u[0], u[1], u[2]);

// reverse — two-pointer swap incl. the absent cases; returns the
// receiver for chaining
var r: any = { 0: 1, 2: 3, length: 3 };
r.reverse = Array.prototype.reverse;
var chained = r.reverse();
console.log(r[0], r[1], r[2], chained === r);

// heap elements through the mutators (rc ledger)
var h: any = { 0: "pear", 1: "fig", length: 2 };
h.pop = Array.prototype.pop;
h.unshift = Array.prototype.unshift;
console.log(h.pop());
console.log(h.unshift("kiwi"));
console.log(h[0], h[1], h.length);
