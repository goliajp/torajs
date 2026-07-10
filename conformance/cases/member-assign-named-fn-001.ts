// chunk 783 — bare named-fn RHS at member-assign sites into fn-typed
// struct fields wraps in a __forward_ closure (previously the assign
// lane stored a raw FnSig and the narrowed call SIGBUSed).
type O = { cb?: () => number };
type R = { cb: () => number };
type V = { cb?: () => void };

function g(): number { return 1 }
function h(): number { return 7 }
function v() { console.log("side") }

const o: O = { cb: undefined };
o.cb = g;
if (o.cb) { console.log(o.cb()) }

const r: R = { cb: g };
r.cb = h;
console.log(r.cb());

const vv: V = {};
vv.cb = v;
if (vv.cb) { vv.cb() }

function run(): number {
  const local: O = {};
  local.cb = h;
  if (local.cb) { return local.cb() }
  return 0
}
console.log(run());
