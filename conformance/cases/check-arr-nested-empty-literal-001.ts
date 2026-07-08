// Chunk 695 — all-empty nested array literals (`[[]]`, `[[], []]`)
// infer Array<Array<Any>> by extending the P0.10 empty-`[]` default
// one level (pre-fix the checker loud-rejected "array of all-empty
// inner literals — cannot infer element type"; bun prints the
// nested multi-line form). Mutation through an element alias keeps
// the shared inner cell live.
console.log([[]]);
console.log([[], []]);
const x = [[]];
console.log(x.length, x[0].length);
const y = [[], []];
const inner = y[1];
inner.push(5);
console.log(y[1].length);
// mixed: empty inner defers to the non-empty sibling (regression —
// pre-existing rule 2 path stays intact)
const m = [[], [7]];
console.log(m[1][0]);
