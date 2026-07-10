// chunk 797 — unregistered fns print as `[Function]`, bun's
// anonymous spelling (node spells `[Function (anonymous)]`; bun is
// the parity oracle). Named registry rows are the control group.
// (An anonymous fn under an OBJECT key displays the key in bun's
// inspect — `{ f: [Function: f] }` — a separate display-inference
// face, archived.)
console.log([() => 3][0]);
console.log([[() => 3][0]]);
function mk(): () => number {
  return () => 9;
}
console.log(mk());
console.log([mk()]);
const g = () => 1;
console.log([g]);
console.log({ g });
console.log(g);
