const a = [1,2][Symbol.iterator]();
console.log(a.next());
function* g() { yield 7; }
const gi = g();
console.log(gi.next());
console.log(gi.next());
const m = new Map([[1,"a"]]).entries();
console.log(m.next());
const s = new Set([5]).values();
console.log(s.next());
const st = "ab"[Symbol.iterator]();
console.log(st.next());
