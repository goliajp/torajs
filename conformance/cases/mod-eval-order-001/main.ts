// §16.2.1.5 — a module body runs only AFTER every module it requests.
// Pre-fix tora drained its worklist breadth-first and injected each
// module's statements in POP order, which is the exact opposite: an
// importer's body landed ahead of the module it imported. The chain
// below printed `mid before / mid after / leaf body`.
//
// Substrate fix (r421): the resolver records requester → requested as
// it walks (whatever a walk pushed onto the worklist IS that module's
// request list, so nothing has to be threaded down into the helpers)
// and splices the held-aside statements back in depth-first
// post-order once the queue drains.
//
// The same reordering closed a second symptom: a lib whose top-level
// code reads a name from ITS import used to be injected ahead of that
// name's declaration and died with `unknown ident`.
import "./mid.ts";
console.log("main");
