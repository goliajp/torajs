// A dynamic-import target that exists but is not module code rejects
// with a SyntaxError (§16.2.1.6 — ParseModule failed), not the
// TypeError a missing module answers with. bun rejects the unparsable
// file and the missing one with its own bundler classes (BuildMessage
// / ResolveMessage, settled in bundler order) — `main.expected` holds
// the spec answer (test262 `eval-script-code-target`); node agrees on
// the SyntaxError.
import("./script-only.js").then(
  () => console.log("script-only resolved"),
  (e: any) => console.log("script-only", e instanceof SyntaxError, e.name),
);
import("./unparsable.js").then(
  () => console.log("unparsable resolved"),
  (e: any) => console.log("unparsable", e instanceof SyntaxError, e.name),
);
import("./missing.js").then(
  () => console.log("missing resolved"),
  (e: any) => console.log("missing", e instanceof TypeError, e.name),
);
