// Phase K.6 — Arr / Obj refcount globals. Extends K.4's Str-only
// support to cover the other ptr-shaped refcount heap types. Same
// shape as K.4: slot is ptr, init must be fresh-heap (function
// return), reads from named-fn bodies are borrows, mutable assign
// is rejected.
//
// emit_drops_for_globals fires `emit_drop_value` per slot at the
// fall-through `main` exit. For Arr<T> with refcounted T it walks
// element-drop first; for Obj it dispatches to the registered
// __torajs_obj_drop chain.

type Pair = { fst: number, snd: number };

function makeNums(): number[] {
  let xs: number[] = [10, 20, 30];
  return xs;
}

function makePair(): Pair {
  return { fst: 7, snd: 11 };
}

const NUMS: number[] = makeNums();
const PAIR: Pair = makePair();

function nthNum(i: number): number {
  return NUMS[i];
}

function pairFst(): number {
  return PAIR.fst;
}

function pairSnd(): number {
  return PAIR.snd;
}

function check(): number {
  if (NUMS.length !== 3) { throw "#1: NUMS.length"; }
  if (NUMS[0] !== 10) { throw "#2: NUMS[0]"; }
  if (nthNum(1) !== 20) { throw "#3: nthNum(1)"; }
  if (nthNum(2) !== 30) { throw "#4: nthNum(2)"; }
  if (PAIR.fst !== 7) { throw "#5: PAIR.fst direct"; }
  if (pairFst() !== 7) { throw "#6: PAIR.fst via fn"; }
  if (pairSnd() !== 11) { throw "#7: PAIR.snd"; }
  return 0;
}
console.log(check());
