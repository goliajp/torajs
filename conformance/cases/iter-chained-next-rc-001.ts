// A chained `xs.entries().next()` mints an iterator that nothing else
// ever drops — the receiver is only borrowed for the step, so the
// `.next()` lowering has to release it. Hoisting the iterator into a
// variable was already correct, which is what pinned the leak to the
// receiver rather than to the IteratorResult.
//
// A leak has no wrong output to assert on, so this fixture pins the
// behaviour that must survive the release: the pair array yielded by
// `entries()` outlives the iterator it came from.

const arr: any[] = [[1], [2], [3]];

const first = arr.entries().next().value;
console.log(first[0], String(first[1]));

// the yielded pair must still be readable after the temp iterator dies
const kept: any[] = [];
for (let i = 0; i < 3; i++) {
  kept.push(arr.entries().next().value);
}
console.log(kept.length, kept[0][0], String(kept[2][1]));

// chained on a Map, and on values()/keys()
const m = new Map<string, number>();
m.set('a', 1);
m.set('b', 2);
console.log(JSON.stringify(m.entries().next().value));
console.log(m.keys().next().value, m.values().next().value);
console.log(arr.keys().next().value, arr.keys().next().done);

// every chained step restarts from index 0 — a fresh iterator each time
console.log(arr.keys().next().value, arr.keys().next().value);

// hoisted iterators keep advancing (unchanged behaviour)
const it = arr.keys();
console.log(it.next().value, it.next().value, it.next().value, it.next().done);
