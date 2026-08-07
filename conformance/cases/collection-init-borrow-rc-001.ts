// rotation 325 — the collection-initializer lanes (Map/Set from a
// typed pair array, Map/Set clone, the general iterable walk, and
// the fromEntries let fast-path + its trailing args) all consumed
// their source argument unconditionally. An owned temp (a literal, a
// call result) that is correct for; an ident-bound BORROW keeps its
// binding's stake, and the unconditional drop stole it — the
// binding's scope-end release then dec'd through freed pair arrays
// (census: zero incs, two decs on the inner pair). Every drop is now
// gated on expr_transfers_ownership, the same predicate the write
// lanes use.
const pairs: Array<Array<number>> = [[1, 10], [2, 20]];
const m1 = new Map(pairs);
console.log(m1.size, m1.get(2), pairs.length);
const m2 = new Map(m1);
console.log(m2.size, m2.get(1), m1.size);
const items: Array<number> = [7, 8, 7];
const s1 = new Set(items);
console.log(s1.size, items.length);
const s2 = new Set(s1);
console.log(s2.size, s1.size);
type Pair = { a: i64, b: i64 };
let o: Pair = { a: 10, b: 20 };
let entries = Object.entries(o);
let back: Pair = Object.fromEntries(entries);
console.log(back.a, back.b, entries.length);
console.log("done");
