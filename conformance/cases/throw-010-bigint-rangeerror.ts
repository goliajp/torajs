// P7.4-a-b — bigint Div/Mod/Pow and an over-large Shl throw a REAL
// catchable RangeError (spec §20.5.5 NativeError), not a process abort
// or bare string, and the RangeError propagates correctly across
// direct try/catch, a named-fn boundary, and transitively through a
// fixed-point may-throw chain.
//
// Asserts only spec-defined facts: instance type, prototype chain
// (RangeError extends Error), .name, catchability, propagation. The
// engine-specific .message text and .constructor identity are NOT
// asserted (the spec mandates the RangeError TYPE; JSC's wording and
// the .constructor wiring are separate concerns / known gaps).
//
// Propagation shapes: direct try/catch and named-fn calls only.

// --- direct try/catch: each throwing bigint op. The `const c = …`
// binding is the #13 entry-hoist scenario (slot must reach the
// post-throw-check scope-end drop). ---
try { const c = 10n / 0n; console.log(c); }
catch (e: RangeError) { console.log("div0 | " + (e instanceof RangeError) + " | " + (e instanceof Error) + " | " + e.name); }

try { const c = 10n % 0n; console.log(c); }
catch (e: RangeError) { console.log("mod0 | " + (e instanceof RangeError) + " | " + (e instanceof Error) + " | " + e.name); }

try { const c = 2n ** -1n; console.log(c); }
catch (e: RangeError) { console.log("negexp | " + (e instanceof RangeError) + " | " + (e instanceof Error) + " | " + e.name); }

// shift amount itself unrepresentable (n->len > 1) → "shift amount
// too large" RangeError guard.
try { const c = 1n << 123456789012345678901234567890n; console.log(c); }
catch (e: RangeError) { console.log("shlbig | " + (e instanceof RangeError) + " | " + (e instanceof Error) + " | " + e.name); }

// --- well-formed bigint arithmetic still works (no spurious throw) ---
const ok = (6n / 2n) + (7n % 3n) + (2n ** 10n) + (1n << 4n);
console.log("ok=" + ok);

// --- named-fn boundary propagation (silently swallowed pre-prefix) ---
function boom(): bigint {
  const z = 0n;
  return 5n / z;
}
try {
  const r = boom();
  console.log("unreached " + r);
} catch (e: RangeError) {
  console.log("named-fn | " + (e instanceof RangeError) + " | " + e.name);
}

// --- transitive propagation through a fixed-point may-throw chain ---
function inner(): bigint { return 9n % 0n; }
function outer(): bigint { return inner() + 1n; }
try {
  const r = outer();
  console.log("unreached " + r);
} catch (e: RangeError) {
  console.log("transitive | " + (e instanceof RangeError) + " | " + e.name);
}
