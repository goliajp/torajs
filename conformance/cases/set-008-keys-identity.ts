// §24.2.4.8 — the initial value of Set.prototype.keys is the same
// function object as Set.prototype.values; Map keeps them distinct.

const sp: any = Set.prototype;
console.log(sp.keys === sp.values, sp.entries === sp.values, sp.keys.name);

const s: any = new Set([7]);
console.log(s.keys === s.values);

const mp: any = Map.prototype;
console.log(mp.keys === mp.values);

// aliased cell still iterates as values
const it = s.keys();
const first = it.next();
console.log(first.value, first.done, it.next().done);
