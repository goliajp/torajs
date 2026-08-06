// A builtin-prototype patch that is not an object is not callable
// (§13.3.6.1 step 5) — and saying so is all this may do with it.
// The prototype own-probe's value channel only carries a pointer
// under the heap tag; under every other tag it is an immediate, so
// reading it as a cell took the process down instead of throwing.

// --- the walk lane's adder (§24.1.1.1 step 7) ---
(Map.prototype as any).set = null;
const mapSrc: any = [[1, 1]];
try {
  const m: any = new Map(mapSrc);
  console.log("map: no throw", m.size);
} catch (e: any) {
  console.log("map:", e instanceof TypeError);
}

(Set.prototype as any).add = 7;
const setSrc: any = [1, 2];
try {
  const s: any = new Set(setSrc);
  console.log("set: no throw", s.size);
} catch (e: any) {
  console.log("set:", e instanceof TypeError);
}

// --- the primitive pre-gate lanes ---
(Number.prototype as any).toFixed = null;
const num: any = 1.5;
try {
  console.log("num: no throw", num.toFixed(1));
} catch (e: any) {
  console.log("num:", e instanceof TypeError);
}

(Boolean.prototype as any).toString = "nope";
const bln: any = true;
try {
  console.log("bool: no throw", bln.toString());
} catch (e: any) {
  console.log("bool:", e instanceof TypeError);
}

// --- an accessor patch answering a non-callable was already right ---
Object.defineProperty(String.prototype, "trimStart", {
  get: function () {
    return 7;
  },
  configurable: true,
});
const acc: any = " x";
try {
  console.log("accessor: no throw", acc.trimStart());
} catch (e: any) {
  console.log("accessor:", e instanceof TypeError);
}

// --- a callable patch keeps working: the guard is about shape ---
let hits = 0;
(String.prototype as any).toUpperCase = function () {
  hits = hits + 1;
  return "PATCHED";
};
const str: any = "abc";
console.log("fn patch", str.toUpperCase(), hits);

// a borrowed builtin cell still coerces through the generic lane
(Number.prototype as any).split = (String.prototype as any).split;
const borrowed: any = 1.5;
console.log("borrowed", JSON.stringify(borrowed.split(".")));
