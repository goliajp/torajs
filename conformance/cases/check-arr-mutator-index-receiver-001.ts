// Chunk 700 — `z[i].pop()` / `z[i].shift()` / `z[i].unshift(v)`
// index-read receivers (the chunk-697 push twin): the mutator lane's
// resolve_mutator_arr_receiver only knew Ident / global / obj.field
// shapes. B1 fixed the arr cell across grow, so the borrowed elem
// read IS the receiver — no outer-slot write-back exists to miss.
const z: number[][] = [[1, 2, 3], [4, 5]];
console.log(z[0].pop());
console.log(z[0].shift());
z[1].unshift(9);
console.log(z);
// spec §23.1.3.34 — unshift answers the new length
const r: number[][] = [[5], [6]];
console.log(r[1].unshift(7));
// empty-array guard rides the same lane (bug-327 C1); typed-tier
// print bar — verify via length, not the returned zero-value
const e: number[][] = [[]];
e[0].pop();
e[0].shift();
console.log(e[0].length);
// refcounted elems
const s: string[][] = [["a", "bb"], ["ccc"]];
console.log(s[0].pop());
s[1].unshift("dd");
console.log(s);
// Any inner arrays route the kind-aware runtime helpers (empty →
// undefined built into arr_any_pop / arr_any_shift). Mixed inner
// literals mint the FLAG_ARR_ANY flavor; a uniform inner literal
// ([2]) minting typed-behind-any makes kind-change unshift hit the
// catchable-TypeError protocol — that mint-flavor gap is L3b, not
// this receiver-shape fixture.
const anyz: any[][] = [[1, "x", true], [2, "w"]];
console.log(anyz[0].pop());
console.log(anyz[0].shift());
anyz[1].unshift("y");
console.log(anyz);
const anyE: any[][] = [[]];
console.log(anyE[0].pop());
console.log(anyE[0].shift());
// non-literal index
const k = 1;
const w: number[][] = [[1], [2, 3]];
console.log(w[k].shift());
console.log(w);
// 1000-elem drain across the deque head-bump
const g: number[][] = [[]];
for (let i = 0; i < 1000; i++) g[0].push(i);
let sum = 0;
for (let i = 0; i < 1000; i++) sum += g[0].shift();
console.log(sum, g[0].length);
