// The cycle collector's class-object arm answered an `any` field's
// raw slot as a child pointer. An immediate lives there as NaN-box
// bits — `undefined` is 0x…0a — so the collect sweep that has no
// cell-like gate of its own dereferenced address 10 and took SIGSEGV
// at exit. Every other shape (dynobj / arr / closure trace) filters
// immediates where it produces them; this arm now does too.
//
// The reachable shape: a self-recursive nested function capturing a
// generator object. The recursion never has to RUN — the call site
// alone routes the capture through the closure lane, which makes the
// pair a cycle the collector walks at exit. The immediate it trips
// on is the generator class's own `__sent` field, which starts
// undefined. Before the fix this printed its line and then died with
// signal 11.
function drive(g: any): void {
  function step(v: number): void {
    const keep: any = g;
    if (v > 0) {
      step(v - 1);
    }
  }
  step(0);
}

function* g1(): any {
  yield 5;
  return 0;
}

drive(g1());
console.log("survived exit");
