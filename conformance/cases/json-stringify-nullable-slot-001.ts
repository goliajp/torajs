// §25.5.2 step 12 on a pointer-shaped slot the checker typed
// `T | undefined`. The slot holds the generic immortal cell for its
// undefined, and the static unfold monomorphizes against the shape
// the slot COULD hold — false exactly when it is holding the cell.
// The struct and class spellings printed whatever the field offset
// landed on; the array one printed an empty string.

// --- the three pointer shapes, holding undefined ---
const s: { a: number } | undefined = undefined;
console.log(JSON.stringify(s));

const xs: number[] | undefined = undefined;
console.log(JSON.stringify(xs));

class C {
  a = 1;
}
const c: C | undefined = undefined;
console.log(JSON.stringify(c));

// the call answers the undefined VALUE, not the text "undefined"
console.log(typeof JSON.stringify(s));

// --- the same slots holding a live value still serialize ---
const s2: { a: number } | undefined = { a: 1 };
console.log(JSON.stringify(s2));

const xs2: number[] | undefined = [1, 2];
console.log(JSON.stringify(xs2));

const c2: C | undefined = new C();
console.log(JSON.stringify(c2));

// ... and under a gap, which the walk indents from level zero
console.log(JSON.stringify(s2, null, 2));

// --- a nullable slot nested inside a composite is unchanged:
// §25.5.2.4 omits an undefined property and prints a null element
console.log(JSON.stringify([s]));
console.log(JSON.stringify({ k: s, j: 1 }));

// --- the lanes that were already right stay on their own arms ---
const str: string | undefined = undefined;
console.log(JSON.stringify(str));
const n: number | undefined = undefined;
console.log(JSON.stringify(n));
console.log(JSON.stringify({ a: 1 }));
console.log(JSON.stringify([1, 2]));
console.log(JSON.stringify("x"));
console.log(JSON.stringify(7));

// a replacer still takes the whole walk into the kernel, and the
// root is offered to it even when the slot is holding undefined
console.log(JSON.stringify(s, (_k, v) => (v === undefined ? 42 : v)));
