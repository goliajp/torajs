// ES §22.2.6.4-10 — RegExp instance flag accessors. `.flags` returns
// the spec-ordered flag string ("" / "g" / "gi" / "gimsuy"). `.global`
// / `.ignoreCase` / `.multiline` / `.dotAll` / `.unicode` / `.sticky`
// each return a boolean for the matching bit. Pre-fix tr rejected
// every accessor except `.source` and `.lastIndex` at the type check
// ("no member `.flags` on type RegExp").

// `.flags` ordering — spec is `g` `i` `m` `s` `u` `y`
console.log(/abc/.flags)        // ""
console.log(/abc/g.flags)       // "g"
console.log(/abc/gi.flags)      // "gi"
console.log(/abc/gim.flags)     // "gim"
console.log(/abc/gimsuy.flags)  // "gimsuy"
console.log(/abc/ig.flags)      // "gi" — input order doesn't matter; output is canonical
console.log(/abc/y.flags)       // "y"

// Boolean accessors
console.log(/abc/.global)        // false
console.log(/abc/g.global)       // true
console.log(/abc/gi.global)      // true
console.log(/abc/.ignoreCase)    // false
console.log(/abc/i.ignoreCase)   // true
console.log(/abc/.multiline)     // false
console.log(/abc/m.multiline)    // true
console.log(/abc/s.dotAll)       // true
console.log(/abc/u.unicode)      // true
console.log(/abc/y.sticky)       // true
console.log(/abc/gim.sticky)     // false

// `.source` unchanged regression guard
console.log(/abc/g.source)       // "abc"
console.log(/(foo|bar)/gi.source)  // "(foo|bar)"

// `.flags` matches `.source` invariance: regex literals with the
// same body but different flag input produce different `.flags`
// outputs in spec-fixed order.
console.log(/x/yu.flags)   // "uy" — input "yu" canonicalised to "uy"
console.log(/x/mig.flags)  // "gim" — input "mig" canonicalised to "gim"
