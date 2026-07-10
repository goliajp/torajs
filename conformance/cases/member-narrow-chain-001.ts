// chunk 789 — member-path narrows key on canonical receiver paths,
// so Member-chain receivers narrow too: `if (h.o.cb) { h.o.cb() }`.
// the named-fn member-assign wrap also resolves chain receivers
// (`h.o.cb = g`). Index receivers (`arr[0].cb`) stay a loud reject —
// the SSA lanes don't reach through element loads yet (archived).
type O = { cb?: () => number };
type H = { o: O };
const h: H = { o: {} };
if (h.o.cb) { console.log(h.o.cb()) } else { console.log("none") }
h.o.cb = () => 5;
if (h.o.cb) { console.log(h.o.cb()) }
function g(): number { return 6 }
h.o.cb = g;
if (h.o.cb) { console.log(h.o.cb()) }
h.o.cb = undefined;
if (h.o.cb) { console.log("bad") } else { console.log("cleared") }
console.log("end");
