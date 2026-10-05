// a symbol description outside Latin-1 is stored UTF-16: toString,
// String(), a computed member's name and console.log all keep it
const s = Symbol("héllo€😀");
console.log(s.description, s.toString(), String(s).length);
console.log(s);
const t = Symbol("abé");
console.log(t.toString(), t.toString().toUpperCase());
console.log(t);
const u = Symbol();
console.log(u.toString(), u);
const o = { [s]() { return 1; } };
console.log(o[s].name);
console.log([s, t]);
