// §13.15.1 — the BoundNames of a destructured CatchParameter are the
// pattern's, not the synthesized `__catch_destr_` holder's. The
// rejects (`catch ([e]) { let e }` and friends) are early errors and
// live in the negative suite; what runs here are the faces that stay
// legal, so the new BoundNames walk is exercised on every shape it
// has to enumerate.
try { throw [1, 2]; } catch ([a, b]) { console.log(a, b); }
try { throw { p: 3 }; } catch ({ p }) { console.log(p); }
try { throw { q: { r: 4 } }; } catch ({ q: { r } }) { console.log(r); }
try { throw [5, 6, 7]; } catch ([h, ...t]) { console.log(h, t.length); }
try { throw { s: 8, u: 9 }; } catch ({ s, ...rest }) { console.log(s, Object.keys(rest).length); }
try { throw []; } catch ([d = 10]) { console.log(d); }
try { throw [[11, 12]]; } catch ([[m, n]]) { console.log(m, n); }
try { throw [[13]]; } catch ([...[k]]) { console.log(k); }

// A different name is untouched, and an inner block may shadow.
try { throw [14]; } catch ([e]) { let f = e + 1; console.log(f); }
try { throw [15]; } catch ([e]) { { let e = 99; console.log(e); } console.log(e); }

// Annex B B.3.4 waives the VarDeclaredNames clause only for a simple
// parameter, and that face keeps working.
try { throw 16; } catch (e) { var e; console.log(e); }
