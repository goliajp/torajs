// §23.1.3.1 — an Any-typed concat argument decides spread-vs-append
// at runtime (IsConcatSpreadable subset: real arrays spread,
// everything else appends as one element). Covers the typed-receiver
// mixed lane and the Arr<Any>-receiver lane (whose old packed append
// silently nested an array argument).

const a1: any = [9, 8];
console.log(JSON.stringify([1].concat(a1)));

const a2: any = 9;
console.log(JSON.stringify([1].concat(a2)));

const a3: any = "s";
console.log(JSON.stringify([1].concat(a3)));

const a4: any = [[2], [3]];
console.log(JSON.stringify([1].concat(a4)));

const a5: any = null;
console.log(JSON.stringify([1].concat(a5)));

// Any-receiver spelling — the latent single-element guess
const xs: any[] = [1, "t"];
const y: any = [2, 3];
console.log(JSON.stringify(xs.concat(y)));
console.log(JSON.stringify(xs.concat(a2)));

// mixed multi-arg fold: any / typed array / scalar in one call
console.log(JSON.stringify([1].concat(a1, [4], 5)));

// owned temp argument (call result transfers ownership)
function mk(): any {
  return [7, 6];
}
console.log(JSON.stringify([1].concat(mk())));

// receiver is never mutated
const base = [1, 2];
base.concat(a1);
console.log(JSON.stringify(base));

// string receiver-element divergence: heterogeneous result reads back
const het = ["a"].concat(a1);
console.log(het[0], het[1], het.length);
