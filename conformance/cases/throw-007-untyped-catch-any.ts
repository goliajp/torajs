// P7.2b-2 — an unannotated `catch (e)` binds Any, per TS spec
// (the catch parameter is implicitly `any`; an explicit
// non-any/unknown annotation is the TS1196 error). Pre-fix tora
// defaulted the untyped catch slot to I64 (a pre-spec M4.1 tora-ism),
// which silently corrupted every non-int throw: a string read back
// as a raw pointer, a float as its f64 bit pattern, an object as a
// pointer-as-int. Only `throw <int>` happened to round-trip.
//
// Fix routes untyped `catch (e)` through the same tag-aware any_box
// reconstruction as `catch (e: any)`. Enabled by P7.2b-1 (Any→number
// at the return/assign sinks) so `catch (e) { return e + n }` and
// `r = e + n` (r a `: number`) still produce the numeric value.
//
// Acceptance: every thrown primitive caught untyped survives intact
// (print, typeof, arithmetic), including the assign-to-numeric-local
// shape (m4-03) and the return-from-numeric-fn shape (m4-02).

// 1. Each primitive type caught untyped prints exactly as bun.
try { throw 3.14; } catch (e) { console.log(e); }      // 3.14
try { throw "boom"; } catch (e) { console.log(e); }    // boom
try { throw 42; } catch (e) { console.log(e); }        // 42
try { throw true; } catch (e) { console.log(e); }      // true

// 2. typeof on the untyped-caught value (tag-correct).
try { throw 7; } catch (e) { console.log(typeof e); }       // number
try { throw "s"; } catch (e) { console.log(typeof e); }     // string
try { throw false; } catch (e) { console.log(typeof e); }   // boolean

// 3. Arithmetic on the untyped-caught value, returned from a
//    `: number` fn (m4-02 shape).
function check(n: number): number {
  if (n < 0) {
    throw 99;
  }
  return n;
}
function safe(n: number): number {
  try {
    return check(n);
  } catch (e) {
    return e + 1000;
  }
}
console.log(safe(-5)); // 1099
console.log(safe(3));  // 3

// 4. Assign the untyped-caught value into a declared `: number`
//    local, read it back through finally + return (m4-03 shape).
function f(n: number): number {
  let r: number = 0;
  try {
    if (n < 0) {
      throw 99;
    }
    r = n + 1;
  } catch (e) {
    r = e + 1000;
  } finally {
    console.log(r);
  }
  return r;
}
console.log(f(7));  // 8 then 8
console.log(f(-5)); // 1099 then 1099
