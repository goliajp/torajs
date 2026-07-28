// t1 — sync gen decl, plain elements
function* g() { yield* [1, 2, 3]; }
for (const x of g()) console.log(x);

// t2 — empty array delegation yields nothing
function* g2() { yield* []; yield 9; }
for (const x of g2()) console.log(x);

// t3 — object elements pass through by reference (the [iter] harness shape)
let obj = { tag: "A" };
function* g3() { yield* [obj]; }
for (const x of g3()) console.log(x.tag);

// t4 — mixed with plain yields before/after
function* g4() { yield 0; yield* [5, 6]; yield 7; }
for (const x of g4()) console.log(x);

// t5 — J.3 typed lane regression: direct call to a known function*
function* inner() { yield 100; yield 200; }
function* g5() { yield* inner(); }
for (const x of g5()) console.log(x);
