// §16.1.7 step 5.c HasRestrictedGlobalProperty — a Script's lexical
// declaration of `undefined` / `NaN` / `Infinity` (the §19.1 value
// properties, non-configurable) is a SyntaxError when the Script is
// evaluated, creating none of its bindings; a Script inside a function
// gets the same answer from the runtime preflight.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`,
// EXCEPT `typeof ok1`: V8 declares a Script's `var`s one by one before the
// failing `let`, §16.1.7 runs every check first — the expected holds the
// spec's answer (`undefined`).
try { __t262_evalScript('let NaN;'); } catch (e) { console.log(e.constructor.name); }
try { __t262_evalScript('var ok1; let Infinity;'); } catch (e) { console.log(e.constructor.name, typeof ok1); }
__t262_evalScript('let fine = 1;');
console.log(fine);
function inner() {
  try { __t262_evalScript('let undefined;'); } catch (e) { return e.constructor.name; }
  return "no throw";
}
console.log(inner());
