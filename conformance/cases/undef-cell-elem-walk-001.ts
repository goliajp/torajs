// An ARRAY ELEMENT may be holding the generic immortal cell — that
// is the legal in-band spelling of `undefined` for a pointer slot,
// so nothing is wrong at the store. The two runtime walks over such
// elements read them as live values of the element type: join ran
// ToString against the cell ("value is not a function on this any
// receiver"), and the JSON unfold walked it as an array (SIGSEGV).
//
// Two places, because the two walks reach the element differently:
// join comes through the one helper that turns a typed slot into an
// AnyValue, and the JSON unfold recurses statically behind a gate
// that already asked "is this pointer NULL".
//
// §23.1.3.18 step 5.c gives undefined the empty string; §25.5.2.4
// step 8.a gives it `null` inside an array.

const xs: number[][] = [];
const r = xs.find((z: number[]) => z.length > 99);

// --- the three spellings that reach the same element slot
console.log(JSON.stringify([r]));
const bound = [r];
console.log(JSON.stringify(bound));
const pushed: number[][] = [];
pushed.push(r);
console.log(JSON.stringify(pushed));

console.log("[" + [r].join(",") + "]");
console.log("[" + bound.join("-") + "]");
console.log("[" + [r].toString() + "]");
console.log(typeof bound[0], bound[0] === undefined, String(bound[0]));
console.log(`${bound[0]}`);

// --- mixed with live elements, and nested a level deeper
const mixed = [[1, 2], r, [3]];
console.log(JSON.stringify(mixed));
console.log(mixed.join("|"));
console.log(JSON.stringify([mixed]));

// --- an object element of the same shape
class P {
  a = 1;
}
const ps: P[] = [];
const pm = ps.find((z: P) => z.a > 99);
console.log(JSON.stringify([pm]));
console.log("[" + [pm].join(",") + "]");

// --- an object PROPERTY holding the cell omits its key instead
console.log(JSON.stringify({ k: r }));
console.log(JSON.stringify({ k: r, j: 1 }));

// --- everything live keeps its own answers, indent included
const live = [[1, 2], [3]];
console.log(JSON.stringify(live), live.join("|"), live.toString());
console.log(JSON.stringify(live, null, 2));
console.log(JSON.stringify([[1], null, [2]]));
console.log([[1], [2]].join(";"));
console.log(JSON.stringify([1, "a", true, null]));
console.log([1, "a", true, null, undefined].join(","));
