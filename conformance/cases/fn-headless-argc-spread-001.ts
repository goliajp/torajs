// A spread call site reaches a head-less fn's real `arguments.length`.
//
// Two passes each stood aside at these sites and the call fell through
// to the runtime spread lane, which cannot box a bare fn name — a loud
// "box_to_any element type FnSig". `apply_spread_args` declines an
// arguments-carrying callee on purpose (its index-read expansion trims
// the list to the declared arity, and the real count dies with the
// trimmed tail), while the forwarder wrap declined it because a
// relay used to lose the count. That second reason expired: the
// `__forward_` relay now carries its OWN runtime argc into a head-less
// callee's hidden slot, so the count survives the hop.
//
// Bodies that read argument VALUES still keep the loud refusal — the
// relay passes only the declared params, so widening those sites would
// answer `arguments[i]` undefined instead of failing.

function describe(a: number) {
  return "n=" + arguments.length + " a=" + a;
}

const two = [7, 8];
console.log(describe(1, ...two));
console.log(describe(...two));
console.log(describe(9));

// Several spreads at once, and a spread ahead of fixed args.
const one = [3];
console.log(describe(...two, ...one));
console.log(describe(0, ...two));

// Two declared params, an empty spread source, and a beyond-arity site.
function pair(a: number, b: number) {
  return arguments.length + "/" + a + "/" + b;
}
const none: number[] = [];
console.log(pair(...none, 1, 2));
console.log(pair(1, 2, 3, 4));

// A zero-param counter — the shape the trimming used to answer 0 for.
function tally() {
  return arguments.length;
}
console.log(tally(...[1, 2, 3, 4, 5]));
console.log(tally());
