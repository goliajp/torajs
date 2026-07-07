// Chunk 655 — JSON.stringify of a NULL Arr slot (exec/match miss,
// Nullable<Arr>) answers the string "null" per §25.5.2. Pre-fix the
// composite arr lane dereferenced NULL loading `len` (SIGSEGV).

// 1) exec miss → null → "null".
const r = /(\d)p/.exec("xp");
console.log(JSON.stringify(r));
console.log(r === null);

// 2) exec hit regression — array walk unchanged.
const h = /(\d)p/.exec("3p");
if (h !== null) {
  console.log(JSON.stringify(h));
}

// 3) miss capture inside a hit result — per-element undefined
// stringifies to null (§25.5.2.4), unchanged.
const m = /a(b)?/.exec("a");
console.log(JSON.stringify(m));

// 4) Ident-bound null reuse.
const n = /z/.exec("q");
console.log(JSON.stringify(n));
console.log(JSON.stringify(n));
