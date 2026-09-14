// A labeled BLOCK holding a yield (or, in an async machine, an
// await): the state machine erases the block, so `break lbl` used to
// survive as a literal naming a label the SSA lowering never saw
// ("`break lbl` to unknown label"). The block now gets an exit state;
// a bare `break` inside it still targets the enclosing loop, and a
// break from inside a try/finally the block holds runs the finally
// (with its own suspend) on the way out.
function* g1(): Generator<number> {
  lbl: { yield 1; if (true) break lbl; yield 99; }
  yield 2;
  outer: { try { yield 3; break outer; } finally { yield 4; } }
  yield 5;
  for (let i = 0; i < 3; i++) {
    blk: { if (i === 1) break; yield 10 + i; if (i === 0) break blk; yield 20 + i; }
  }
  yield 6;
}
console.log("g1", [...g1()].join(","));

async function af(): Promise<string> {
  var seen = "";
  lbl: { seen += "body"; if (seen.length > 0) break lbl; await null; seen += ",never"; }
  seen += ",after";
  return seen;
}
async function af2(): Promise<string> {
  let log = "";
  outer: { try { log += "t"; await null; break outer; } finally { log += "f"; await null; log += "F"; } }
  return log + "!";
}
async function af3(): Promise<string> {
  let log = "";
  for (const i of [0, 1, 2]) {
    blk: { if (i === 1) break; await null; log += "a" + i; if (i === 0) break blk; log += "b" + i; }
  }
  return log;
}
af().then((v: any) => console.log("af", v));
af2().then((v: any) => console.log("af2", v));
af3().then((v: any) => console.log("af3", v));
