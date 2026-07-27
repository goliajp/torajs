// RFC 20260727-dstr-decl-shape 刀 B — for-await declaration-head
// patterns (the test262 async-func-dstr-{var,let,const} family's
// shape). Before this blade every non-flat pattern head under
// `for await` fell through the decl scan and died on the
// "requires the iterable form" refusal.

async function flatNames() {
  for await (const [a, b] of [[1, 2], [3, 4]]) {
    console.log(a, b);
  }
}

async function withDefaults() {
  // the test262 flagship shape: defaults over holes / short tuples
  for await (const [v2 = 10, vNull = 11, vHole = 12] of [[2, null]]) {
    console.log(v2, vNull, vHole);
  }
}

async function nested() {
  for await (const [[m, n] = [4, 5]] of [[]]) {
    console.log(m, n);
  }
}

// Recorded boundary (hole Z, plan-state L3b): object patterns under
// `for await` die on the element unwrap — `.value` has no arm for a
// non-Promise Struct element (`for await ({ x: o.x } of [{ x: 5 }])`
// shows the same wall with no pattern machinery involved). The obj
// face joins this fixture once that arm lands.

async function restTail() {
  for await (const [head, ...rest] of [[1, 2, 3]]) {
    console.log(head, rest.length, rest[1]);
  }
}

async function main() {
  await flatNames();
  await withDefaults();
  await nested();
  await restTail();
}
main();
