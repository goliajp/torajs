// RC-4 F4 (RFC 20260706-test262-bug-corpus) — Object.freeze on an
// Any-typed argument must route through the NaN-box-aware
// __torajs_obj_freeze_any. The raw obj_freeze derefs its arg as a
// heap header; an Any sentinel (undefined from builtin-prototype
// method reflection) SIGSEGVd. test262
// built-ins/Array/prototype/forEach/S15.4.4.18_A{1,2} cover this.

// Builtin-prototype method reflection evaluates to an Any (undefined
// in the subset) — freeze must pass it through, not deref.
Object.freeze(Array.prototype.forEach)
console.log('ok-1')

// Freeze inside an in-progress forEach — the exact test262 shape.
let seen = 0
;['z'].forEach(function (): void {
  Object.freeze(Array.prototype.forEach)
  seen = seen + 1
})
console.log(seen)

// Any-boxed heap value still freezes for real through the same
// NaN-box-aware entry.
let o: any = { a: 1 }
Object.freeze(o)
console.log(Object.isFrozen(o))
