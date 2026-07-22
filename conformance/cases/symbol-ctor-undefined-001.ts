// §20.4.1.1 step 2 — Symbol(undefined) means no description,
// identical to Symbol(). The argument is still evaluated (its side
// effects run); only the descriptor value is dropped.
const a = Symbol(undefined);
console.log(a.toString(), a.description === undefined);

const b = Symbol();
console.log(b.toString(), b.description === undefined);

// explicit string description still works (regression guard)
const c = Symbol("tag");
console.log(c.toString(), c.description);

// the arg expression is evaluated for side effects even when undefined
// (`void X` runs X, yields undefined)
const d = Symbol(void console.log("evaluated"));
console.log(d.toString(), d.description === undefined);
