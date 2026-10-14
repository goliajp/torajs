// §19.2.1.3 step 5: a global eval whose text declares a function over
// one of the program's own top-level lexical names throws a SyntaxError
// and binds nothing. The lexical name may be any of several a single
// `let` / `const` declares, or one a destructuring pattern binds. Before
// its declaration the name is in its TDZ, and the error names it as
// written.
// oracle: node script goal (`vm.runInThisContext`); bun runs a `.cts`
// as CommonJS, where top-level `var` is not global.
function r() { return f1; }
function shown(e) { return e.constructor.name + " " + e.message.split("'")[1]; }
try { r(); } catch (e) { console.log(shown(e)); }
try { f1; } catch (e) { console.log(shown(e)); }
try { f1 = 3; } catch (e) { console.log(shown(e)); }
try { typeof f1; } catch (e) { console.log(shown(e)); }
let a = 0, f1 = 5;
try { eval("function f1() { return 1; }"); } catch (e) { console.log(e.constructor.name); }
console.log(typeof f1, f1, a, r());
const [f2] = [() => 2];
try { eval("function f2() { return 1; }"); } catch (e) { console.log(e.constructor.name); }
try { (0, eval)("var f2;"); } catch (e) { console.log(e.constructor.name); }
console.log(f2());
const c = 1, { f3 } = { f3: 3 };
try { (0, eval)("function f3() { return 1; }"); } catch (e) { console.log(e.constructor.name); }
console.log(f3, c);
