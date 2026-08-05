// RFC 20260805-async-fn-state-machine D0 — a generator's lifted local
// keeps the type its initializer says it has.
//
// `desugar_generators` lifts every `let` in a generator body to a
// field of the synthesized `__Gen_*` class so the binding survives a
// yield boundary. An unannotated one used to be pinned to `number`,
// which is right for a loop counter and wrong for everything else a
// body can hold — `const xs = [1, 2]` inside a `function*` did not
// compile at all ("field is Number, value is Array(Number)").
//
// A: the shapes the sniff reads — literals, params, an earlier local,
//    a call to a function that declared its return type.
// B: the carve-outs that outrank the sniff still hold — a for-of
//    binding and a yield resumption ride the any lane.
// C: a loop counter is still a number, which is what the old constant
//    was actually right about.

function mkNum(): number {
  return 7;
}
function mkStr(): string {
  return "ab";
}

function* a(p: string, q: number): any {
  const xs = [1, 2, 3];
  yield xs.length;
  const s = "hi";
  yield s;
  const up = p.toUpperCase();
  yield up;
  const sum = q + 1;
  yield sum;
  const chained = up + "!";
  yield chained;
  const called = mkNum();
  yield called;
  const len = mkStr().length;
  yield len;
  const flag = 1 < 2;
  yield flag;
  return 0;
}

function* b(): any {
  const src = ["x", "y"];
  for (const e of src) {
    yield e;
  }
  const sent = yield "ask";
  yield sent;
  return 0;
}

function* c(): any {
  let i = 0;
  while (i < 3) {
    const doubled = i * 2;
    yield doubled;
    i = i + 1;
  }
  return 0;
}

function drain(g: any, send: any): void {
  let r: any = g.next();
  while (r.done === false) {
    console.log(r.value);
    r = g.next(send);
  }
}

drain(a("hi", 10), 0);
console.log("--");
drain(b(), 99);
console.log("--");
drain(c(), 0);
