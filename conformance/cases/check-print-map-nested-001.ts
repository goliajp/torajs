// inspect wrap trunk chunk E — Map / Set indent threading: nested
// composites inside Map values pad by depth, and a Map nested in an
// array pads its rows at the element indent + 2 (probed against bun
// 1.3.14). Top-level Map/Set shapes are unchanged (locked by the
// existing map/set fixtures).
const m = new Map();
m.set("k", { a: 1 });
m.set("j", [1, 2]);
console.log(m);
console.log([m]);
const s = new Set();
s.add("x");
s.add(2);
console.log(s);
