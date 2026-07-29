// RFC 20260729-fn-value-any V4 刀 2 — ES NamedEvaluation for
// generator function EXPRESSIONS. The hoist pass rewrites each one
// into a top-level `__genexpr_N` decl, which erased the syntactic
// position pass-2B recovers names from, so `.name` used to answer the
// synthetic mint in every position. The hoist now resolves the name
// itself: a self-name wins (§15.5.5 — NamedEvaluation applies only to
// ANONYMOUS definitions), else the declaration binder (§14.3.1.2) /
// assignment Ident target (§13.15.2) / property key (§13.2.5.5) /
// destructuring binder (§8.4.5), else the empty ES name.
function fa({ gen = function* () {} }: any) {
  console.log("dstr-anon", gen.name);
}
fa({});

function fb({ xGen = function* x() {} }: any) {
  console.log("dstr-named", xGen.name);
}
fb({});

let g: any = function* () {};
console.log("let-anon", g.name);

let h: any = function* y() {};
console.log("let-named", h.name);

let o: any = { cb: function* () {} };
console.log("objlit", o.cb.name);

let a: any = null;
a = function* () {};
console.log("assign", a.name);

// No naming position at all — the empty ES name, which the print face
// renders as the anonymous form.
console.log("bare", (function* () {}).name === "");

// A generator DECLARATION keeps its own name (it never went through
// the hoist).
function* decl() {}
console.log("decl", decl.name);
