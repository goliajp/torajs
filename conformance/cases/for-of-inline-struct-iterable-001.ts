// `for..of` over an inline struct — an object literal carrying its
// own `[Symbol.iterator]`, reached through a typed slot (here a
// specialized `src: any` parameter). No class to look the method up
// on, so the source boxes into the any lane, whose runtime
// GetIterator reads the symbol-keyed field; a literal without one is
// §7.4.3's TypeError. Before this the shape was a compile refusal.
function drain(src: any) { const out: any[] = []; for (const x of src) { out.push(x); } return out; }
const it = { [Symbol.iterator]() { let i = 0; return { next: () => (i < 3 ? { value: i++ * 10, done: false } : { value: undefined, done: true }) }; } };
console.log(drain(it));
const late = { [Symbol.iterator]() { let i = 0; return { next() { if (i++ < 1) return { value: 1, done: false }; throw new Error("late"); } }; } };
try { drain(late); console.log("no throw"); } catch (e: any) { console.log("caught", e.message); }
const plain = { a: 1 };
try { drain(plain); console.log("no throw plain"); } catch (e: any) { console.log("caught plain", e instanceof TypeError); }
function sum(src: any) { let s = 0; for (const x of src) { s += x; } return s; }
console.log(sum(it), sum([1, 2, 3]), sum("12".split("").map((c) => +c)));
