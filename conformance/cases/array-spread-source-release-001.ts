// Rotation 543 — the typed spread assembler copied out of every
// source and gave none of them back. `arr_extend_unchecked` reads the
// source's slots and `emit_adopt_copied_range` takes its own +1 per
// copied element, so settling the source's stake was entirely the
// caller's job, and nothing did it. The any-typed assembler already
// did this accounting behind its `was_any` flag; the typed one had no
// equivalent.
//
// Two shapes needed different answers. A source the program can still
// name is released only when the expression was an owned temp
// (`[...[1, 2, 3]]` yes, `[...a]` no). A source this lane MINTED — a
// string, a Substr or a Set walked into a fresh array — has no other
// owner at all, and `release_owned_temp` cannot serve it because the
// ExprId describes the string, not the array that replaced it. That
// second shape is why a BOUND string leaked as much as a temp one.
//
// 200k churn, AOT product RSS, 1.51 MB flat baseline:
//   [..."a"]                20.91 MB -> 1.75 MB
//   [..."abc"]              46.61 MB -> 1.75 MB
//   [..."abcdef"]           65.88 MB -> 1.74 MB
//   const s = "abc"; [...s] 46.61 MB -> 1.75 MB
//   [...[1, 2, 3]]          27.21 MB -> 1.59 MB
//   [...new Set([1,2,3])]  156.04 MB -> 2.02 MB
//
// The gate sees none of that. What it can see is the other side of a
// release: the result is still right, and a bound source is still
// alive and usable after the spread reads it.
console.log([..."abc"]);
console.log([..."👋a"].length, [..."👋a"]);
console.log([...[1, 2, 3]]);

const s = "hello";
console.log([...s].join("-"), s, s.length, s.toUpperCase());
console.log([...s, "z"]);

const a = [1, 2];
console.log([...a, 3], a, a.length);
console.log([...a], [...a]);

const st = new Set([1, 2, 2]);
console.log([...st], st.size);

const m = new Map([[1, "a"]]);
console.log([...m], m.size);

console.log([..."a,b".split(",")]);
