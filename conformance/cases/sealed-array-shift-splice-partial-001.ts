// §23.1.3.29 shift and §23.1.3.31 splice Set the surviving elements one
// slot down FIRST and only then DeletePropertyOrThrow the last index.
// A seal leaves elements writable but not configurable, so the moves
// succeed and the delete fails: node and bun answer
// Object.seal([3,1,2]).shift() with a TypeError and the array left as
// [1,2,2], length 3 — the spec's partial mutation, reproduced. A
// shrinking splice is the same shape and fails BEFORE its insert
// (seal([3,1,2]).splice(0,2,9) is [3,2,2], not [9,2,2]); a growing one
// needs a new index and refuses at entry under seal AND
// preventExtensions, untouched; an equal-count one only Sets existing
// indexes and is legal. Before this, tr shifted and spliced a sealed
// array as if it were plain — 14 of the first 48 cells were wrong.
//
// The second 48 cells spell the receiver STATICALLY (a `number[]` local
// and an `any[]` local): that is the typed lowering's lane — shift's
// inline lock guard and the splice sites — which the `a: any` dispatch
// above never reaches.
function t(tag: string, mode: string, typed: boolean, mk: () => any, f: (a: any) => unknown) {
  const a: any = mk();
  if (mode === "seal") Object.seal(a); else if (mode === "ne") Object.preventExtensions(a); else if (mode === "freeze") Object.freeze(a);
  try { const r = f(a); console.log(mode, tag, typed ? "num" : "any", "NO-THROW", JSON.stringify(r), JSON.stringify(a), a.length); }
  catch (e) { console.log(mode, tag, typed ? "num" : "any", "threw", (e as Error).constructor.name, JSON.stringify(a), a.length); }
}
for (const typed of [false, true]) for (const mode of ["seal", "ne", "plain"]) {
  const mk3 = () => (typed ? ([3, 1, 2] as number[]) : ([3, 1, 2] as any[]));
  const mk1 = () => (typed ? ([7] as number[]) : ([7] as any[]));
  const mk0 = () => (typed ? ([] as number[]) : ([] as any[]));
  t("shift3", mode, typed, mk3, (a: any) => a.shift());
  t("shift1", mode, typed, mk1, (a: any) => a.shift());
  t("shift0", mode, typed, mk0, (a: any) => a.shift());
  t("splice(0,1)", mode, typed, mk3, (a: any) => a.splice(0, 1));
  t("splice(0,1,x)", mode, typed, mk3, (a: any) => a.splice(0, 1, 9));
  t("splice(0,0,x)", mode, typed, mk3, (a: any) => a.splice(0, 0, 9));
  t("splice(0,2,x)", mode, typed, mk3, (a: any) => a.splice(0, 2, 9));
  t("splice(1,1)", mode, typed, mk3, (a: any) => a.splice(1, 1));
}

function lock(a: unknown, mode: string) { if (mode === "seal") Object.seal(a); else if (mode === "ne") Object.preventExtensions(a); }
function ok(tag: string, mode: string, lane: string, a: unknown, r: unknown, len: number) { console.log(mode, tag, lane, "NO-THROW", JSON.stringify(r), JSON.stringify(a), len); }
function no(tag: string, mode: string, lane: string, a: unknown, e: unknown, len: number) { console.log(mode, tag, lane, "threw", (e as Error).constructor.name, JSON.stringify(a), len); }
for (const mode of ["seal", "ne", "plain"]) {
  { const a: number[] = [3, 1, 2]; lock(a, mode); try { const r = a.shift(); ok("shift3", mode, "num-static", a, r, a.length); } catch (e) { no("shift3", mode, "num-static", a, e, a.length); } }
  { const a: number[] = [7]; lock(a, mode); try { const r = a.shift(); ok("shift1", mode, "num-static", a, r, a.length); } catch (e) { no("shift1", mode, "num-static", a, e, a.length); } }
  { const a: number[] = []; lock(a, mode); try { const r = a.shift(); ok("shift0", mode, "num-static", a, r, a.length); } catch (e) { no("shift0", mode, "num-static", a, e, a.length); } }
  { const a: number[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 1); ok("splice(0,1)", mode, "num-static", a, r, a.length); } catch (e) { no("splice(0,1)", mode, "num-static", a, e, a.length); } }
  { const a: number[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 1, 9); ok("splice(0,1,x)", mode, "num-static", a, r, a.length); } catch (e) { no("splice(0,1,x)", mode, "num-static", a, e, a.length); } }
  { const a: number[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 0, 9); ok("splice(0,0,x)", mode, "num-static", a, r, a.length); } catch (e) { no("splice(0,0,x)", mode, "num-static", a, e, a.length); } }
  { const a: number[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 2, 9); ok("splice(0,2,x)", mode, "num-static", a, r, a.length); } catch (e) { no("splice(0,2,x)", mode, "num-static", a, e, a.length); } }
  { const a: number[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(1, 1); ok("splice(1,1)", mode, "num-static", a, r, a.length); } catch (e) { no("splice(1,1)", mode, "num-static", a, e, a.length); } }
  { const a: any[] = [3, 1, 2]; lock(a, mode); try { const r = a.shift(); ok("shift3", mode, "any-static", a, r, a.length); } catch (e) { no("shift3", mode, "any-static", a, e, a.length); } }
  { const a: any[] = [7]; lock(a, mode); try { const r = a.shift(); ok("shift1", mode, "any-static", a, r, a.length); } catch (e) { no("shift1", mode, "any-static", a, e, a.length); } }
  { const a: any[] = []; lock(a, mode); try { const r = a.shift(); ok("shift0", mode, "any-static", a, r, a.length); } catch (e) { no("shift0", mode, "any-static", a, e, a.length); } }
  { const a: any[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 1); ok("splice(0,1)", mode, "any-static", a, r, a.length); } catch (e) { no("splice(0,1)", mode, "any-static", a, e, a.length); } }
  { const a: any[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 1, 9); ok("splice(0,1,x)", mode, "any-static", a, r, a.length); } catch (e) { no("splice(0,1,x)", mode, "any-static", a, e, a.length); } }
  { const a: any[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 0, 9); ok("splice(0,0,x)", mode, "any-static", a, r, a.length); } catch (e) { no("splice(0,0,x)", mode, "any-static", a, e, a.length); } }
  { const a: any[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(0, 2, 9); ok("splice(0,2,x)", mode, "any-static", a, r, a.length); } catch (e) { no("splice(0,2,x)", mode, "any-static", a, e, a.length); } }
  { const a: any[] = [3, 1, 2]; lock(a, mode); try { const r = a.splice(1, 1); ok("splice(1,1)", mode, "any-static", a, r, a.length); } catch (e) { no("splice(1,1)", mode, "any-static", a, e, a.length); } }
}
