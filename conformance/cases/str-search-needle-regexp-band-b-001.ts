// §22.1.3.{7,8,23} step 3-5 — the needle slot of startsWith / endsWith /
// includes runs IsRegExp BEFORE ToString, and a regex raises a TypeError.
// The type is known while compiling, so this is band B: the program is an
// executable that throws, NOT a refusal. tr used to answer
// "argument 0: expected String, got RegExp" and produce nothing at all.

function must_throw(label: string, f: () => void): void {
  try {
    f();
    console.log(label + ": no-throw");
  } catch (e) {
    console.log(label + ": " + String(e));
  }
}

const re = /a/;
must_throw("startsWith", () => { "ab".startsWith(re); });
must_throw("endsWith", () => { "ab".endsWith(re); });
must_throw("includes", () => { "ab".includes(re); });

// §7.1.17 — ToString of a Symbol throws, and that too is settled by the
// type alone. Same band, different step of the same slot.
const sym = Symbol("s");
must_throw("sym-startsWith", () => { "ab".startsWith(sym); });
must_throw("sym-endsWith", () => { "ab".endsWith(sym); });
must_throw("sym-includes", () => { "ab".includes(sym); });

// §13.3.6.1 evaluates the arguments before the call, so a decided throw
// does not swallow their side effects. `pos` runs, then the raise happens.
let ran = 0;
function pos(): number { ran = ran + 1; return 0; }
must_throw("arg-eval", () => { "ab".startsWith(re, pos()); });
console.log("position arg evaluated: " + String(ran));

// Control — the slot still works for every needle the spec does NOT
// reject, so band B did not widen into a blanket refusal.
console.log("ab".startsWith("a"));
console.log("ab".endsWith("b"));
console.log("ab".includes("b"));
console.log("ab".startsWith("b", 1));
