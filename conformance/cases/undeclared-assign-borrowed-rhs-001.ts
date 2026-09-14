// §13.15.2 — the rhs of an assignment to an unresolvable name is
// evaluated before the ReferenceError; when that rhs is a BORROW (a
// local's load, the materialized `arguments`, a param) the throw site
// must not release it — the binding still owns it and drops it on the
// way out. Releasing it twice handed the block to the pooled allocator,
// where the error message string picked it up and the second drop
// decremented the message's first byte (`zzz` read back as `yzz`).
function wArgs(a: number, b: number) { zzz = arguments; return a + b; }
function wLocal() { const arr = [1, 2]; zzz = arr; }
function wParam(s: string) { zzz = s; }
function wFresh(s: string) { zzz = s + "!"; }
for (let i = 0; i < 3; i++) {
  try { wArgs(i, 1); } catch (e) { console.log("args", i, (e as any).message); }
}
try { wLocal(); } catch (e) { console.log("local", (e as any).message); }
try { wParam("k"); } catch (e) { console.log("param", (e as any).message); }
try { wFresh("k"); } catch (e) { console.log("fresh", (e as any).message); }
console.log("done");
