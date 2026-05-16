// T-46 — labeled statement parse. Pre-fix the parser hit "expected
// expression, got Colon at..." on any `label: stmt`. JS spec §13.13
// permits a label on any statement; tora subset doesn't track labels
// for break/continue (still parsed as bare), so the minimal handling
// is to strip leading `Ident COLON` chains at parse_stmt and recurse
// into the inner statement. Stacked labels flatten naturally via the
// recursive call. Unblocks 1 test262 case under
// annexB/language/statements/labeled.

function g(): void { console.log("g") }
function f(): void { console.log("f") }
function h(): void { console.log("h") }

// Single label on a call statement.
greet: g();

// Stacked labels (annexB pattern).
outer: inner: f();

// Label on a block.
blk: {
  console.log("inside-block");
}

// Label inside a function body.
function wrap(): void {
  loop_label: for (let i: number = 0; i < 2; i = i + 1) {
    console.log(i);
  }
}
wrap();

// Label on an empty statement (no-op).
empty_label: ;
h();
