// Chunk 702 — TS contextual typing for array-literal inits: the
// annotation propagates through nested literals, so a kind-uniform
// inner literal under an Any-elem annotation mints the FLAG_ARR_ANY
// flavor. Before this, `[[2]]` under any[][] minted typed-behind-any
// and a kind-change mutator hit the catchable-TypeError protocol
// that exists to protect typed-ALIAS any-views (a literal has no
// typed alias — bun's accept semantics apply).
const anyz: any[][] = [[2]];
anyz[0].unshift("y");
console.log(anyz);
// one-level form
const one: any[] = [2];
one.unshift("x");
one.push(true);
console.log(one);
// fn-scope lane
function f(): void {
  const inner: any[][] = [[3], [4]];
  inner[1].push("q");
  console.log(inner);
}
f();
// three levels down
const deep: any[][][] = [[[1]]];
deep[0][0].unshift("d");
console.log(deep);
// read paths on the contextually-typed block
const r: any[][] = [[5, 6]];
console.log(r[0][0], r[0].length);
console.log(r[0].pop());
// a non-Any annotation is inference-equal — typed lane unchanged
const t: number[][] = [[8]];
t[0].unshift(7);
console.log(t);
