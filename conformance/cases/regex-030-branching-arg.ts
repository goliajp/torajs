// Chunk 656 — re.test/exec/toString with a BRANCHING argument
// expression (ternary). The lower snapshotted cur_block before the
// args were lowered; a ternary arg splits blocks, so the regex call
// was appended into the already-terminated pre-branch block and
// executed before the branch with the merge block's (uninitialized)
// operand — garbage haystack pointer: ~940B/iter RSS leak on the
// churn probes and SIGBUS on the all-hit shape.

let hits = 0;
let total = 0;
for (let i = 0; i < 10; i++) {
  const m = /(\d+)px/.exec(i % 2 === 0 ? "30px" : "xxpx");
  if (m !== null) {
    hits++;
    total += m[1].length;
  }
}
console.log(hits, total);

let t = 0;
for (let i = 0; i < 10; i++) {
  if (/(\d+)px/.test(i % 2 === 0 ? "30px" : "31px")) t++;
}
console.log(t);

// toString with a branching (side-effect) trailing arg.
let k = 0;
console.log(/ab/g.toString(k === 0 ? (k = 1) : (k = 2)));
console.log(k);
