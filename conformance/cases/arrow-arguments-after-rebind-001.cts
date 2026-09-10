// §9.4.4 — an arrow's `arguments` is the enclosing function's binding,
// read live. tr aliases that binding into `__arrowargs` at body entry
// (`alias_arrow_arguments`), and a sloppy `arguments = v` (§13.15.1)
// rebound the local without moving the alias, so an arrow read after
// the rebind answered the pre-rebind object: the call's count where
// node answers the new value's. A rebind now moves the alias in the
// same expression. `.expected` because bun's TypeScript front end
// rejects the assignment as an invalid target even in a sloppy .cts;
// node on the same source as .js is the oracle behind the file.
function j1(x) { const g = () => arguments.length; arguments = [1, 2, 3, 4]; return g(); }
function j2(x) { const g = () => arguments[0]; const before = g(); arguments = [9]; return [before, g()]; }
function j3(x) { const g = () => arguments; arguments = 7; return [g(), typeof g()]; }
function j4(x) { const g = () => arguments.length; const r = []; r.push(g()); arguments = []; r.push(g()); arguments = [1]; r.push(g()); return r; }
function j5(x) { const g = () => (() => arguments.length)(); arguments = "abc"; return g(); }
console.log(j1(1, 2, 3), j2(5, 6), j3(1), j4(1, 2), j5(1));
