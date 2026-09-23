// A binding holding the out-of-range sentinel keeps it across every
// boundary below. These all answer right today; the file exists
// because the answer is easy to lose — the sibling shapes that DO lose
// it (a `.then` callback parameter, a HOF element parameter, a for-of
// loop variable) go through kernels that hand the value over without
// the taint, and a fix there must not cost these.
const zs: number[] = [];

const u: number = zs[9];

function h(p: number) {
  console.log("param", typeof p, p === undefined);
}
h(u);
h(zs[9]);

const o = { v: u };
console.log("field", typeof o.v, o.v === undefined);

let w: number = 1;
w = zs[9];
console.log("assigned", typeof w, w === undefined);

// A binding holding a live value keeps the plain number lane.
const live: number = 5;
h(live);
console.log("live", typeof live, live + 1);
