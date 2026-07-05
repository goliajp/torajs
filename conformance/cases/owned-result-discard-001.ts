// RFC 20260705 owned-result invariant: statement-position Call/New
// results are unconditionally released (blanket discard replaced the
// chunk-542 whitelist). Covers the former whitelist entries plus the
// newly-owned borrow-site results and namespace statics (542 gate
// regression shape).
let a = [3, 1, 2];
a.reverse();
console.log(a[0]);
a.sort();
console.log(a[0]);
a.fill(7, 0, 1);
console.log(a[0]);
a.copyWithin(0, 1);
console.log(a[0]);
let m = new Map<string, number>();
m.set("k", 1);
m.set("j", 2);
console.log(m.size);
let st = new Set<number>();
st.add(5);
console.log(st.size);
let o = { x: 1 };
Object.freeze(o);
console.log(Object.isFrozen(o));
let strs = ["b", "a"];
strs.sort();
console.log(strs[0]);
strs.valueOf();
console.log(strs.length);
// fresh results discarded in statement position
strs.slice(0, 1);
strs.toReversed();
console.log(strs[0]);
