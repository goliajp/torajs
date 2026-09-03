// §23.1.3.18 step 5.d is ToString(element), and that runs user code:
// an own `toString` can throw anything, and a Symbol element is
// rejected by §7.1.17 step 2 on its own. The join walk did not ask
// whether a throw was pending, so it finished the loop, handed back a
// string, and the element's exception was swallowed — after calling
// more user methods that the spec says never run.
const sym: any[] = [Symbol("q")];
try { sym.join(","); } catch (e) { console.log("join", (e as Error).constructor.name); }
try { String(sym); } catch (e) { console.log("String", (e as Error).constructor.name); }

const boom: any[] = [{ toString() { throw new RangeError("boom"); } }];
try { boom.join(","); } catch (e) { console.log("join", (e as Error).constructor.name); }
try { String(boom); } catch (e) { console.log("String", (e as Error).constructor.name); }
try { console.log("" + boom); } catch (e) { console.log("concat", (e as Error).constructor.name); }

// the walk stops where the spec stops — the second element never runs
let ran = 0;
const order: any[] = [
  { toString() { ran++; throw new RangeError("first"); } },
  { toString() { ran++; return "second"; } },
];
try { order.join(","); } catch (e) { console.log("ran", ran, (e as Error).constructor.name); }

// a symbol in the middle rejects the whole join, not just its slot
const mid: any[] = [1, Symbol("q"), 2];
try { console.log(mid.join("-")); } catch (e) { console.log("mid", (e as Error).constructor.name); }

// lanes that never run user code are unchanged
console.log([1, 2, 3].join("-"), ["a", "b"].toString(), String([true, false]));
const ok: any[] = [1, "a", null, undefined, { x: 1 }];
console.log(ok.join("-"));
const nested: number[][] = [[1], [2, 3]];
console.log(nested.join(","), String(nested));
