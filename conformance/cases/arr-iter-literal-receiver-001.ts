// arr iter ctor on literal receivers — the create helper takes its
// own +1 on the source array, so the literal temp's stake must be
// released after the call (leak regression: 200k-churn `[1,2].values()`
// parked every arr cell at rc=1, +25.7MB RSS).
const it1: any = [10, 20].values();
console.log(it1.next().value, it1.next().value, it1.next().done);
const it2: any = [7, 8, 9].keys();
console.log(it2.next().value, it2.next().value);
const it3: any = [["a"], ["b"]].entries();
const e: any = it3.next().value;
console.log(e[0], e[1][0]);
// variable receiver — release must no-op (ident is a borrow); the
// binding keeps its own stake and stays alive after iteration.
const arr = [1, 2, 3];
const it4: any = arr.values();
console.log(it4.next().value);
console.log(arr.length);
// spread over a literal-receiver iterator (exhaustion latch drops
// the iter's ref; the literal's own stake was already released).
console.log([...[5, 6].values()].length);
