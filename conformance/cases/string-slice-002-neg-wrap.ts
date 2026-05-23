// P3.1-g.5 — exercise __torajs_str_slice negative-wrap + clamp.
console.log("hello".slice(1, 4));      // ell
console.log("hello".slice(0, 5));      // hello
console.log("hello".slice(-3));        // llo (single-arg form, end=len)
console.log("hello".slice(-3, 5));     // llo
console.log("hello".slice(-3, -1));    // ll
console.log("hello".slice(3, 1));      // (empty — slice does NOT swap)
console.log("hello".slice(20));        // (empty — start past len)
console.log("hello".slice(0, 100));    // hello (end clamped)
console.log("hello".slice(-100, 5));   // hello (start wrap saturates)
console.log("".slice(0, 5));           // (empty s)
