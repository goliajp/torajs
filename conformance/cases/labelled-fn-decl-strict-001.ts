// §14.13.1 — `LabelledItem : FunctionDeclaration` is a Syntax Error in
// strict mode code (Annex B §B.3.2 lifts it for sloppy code only). A
// module is strict, and so is a direct eval called from it, so the
// shape is refused in every one of these positions. The accepting
// side lives in `labelled-fn-decl-hoist-001.cts`.

function report(tag: string, e: unknown) {
  console.log(tag, (e as Error).constructor.name);
}
try { eval("l: function f() {}"); console.log("top", "accepted"); } catch (e) { report("top", e); }
try { eval("a: b: function f() {}"); console.log("chain", "accepted"); } catch (e) { report("chain", e); }
try { eval("{ l: function f() {} }"); console.log("block", "accepted"); } catch (e) { report("block", e); }
try { eval("(function () { l: function f() {} })"); console.log("body", "accepted"); } catch (e) { report("body", e); }

// a label over anything that is not a function declaration stays legal
try { eval("l: (function f() {});"); console.log("labelled-expr", "accepted"); } catch (e) { report("labelled-expr", e); }
try { eval("l: { function f() {} }"); console.log("labelled-block", "accepted"); } catch (e) { report("labelled-block", e); }
