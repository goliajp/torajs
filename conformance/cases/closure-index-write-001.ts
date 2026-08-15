// 405-01 residue — an indexed write on a closure receiver through any
// rides the member-set route (decimal key, §7.1.19), landing in the
// +24 expando dict; pre-fix it fell to the loud not-yet-implemented
// tail while the matching read already resolved. Plus the integrity
// gate this exposed: freeze/seal/preventExtensions mark only the
// receiver HEADER for closure/promise cells, and the expando write
// never consulted it — a frozen function stayed mutable while
// Object.isFrozen answered true (silent-wrong, closed same knife).

function F() {}
const f: any = F
f[5] = "v"
console.log(f[5])
f[5] = "w"
console.log(f[5])
console.log(f.length)
const ar: any = () => 1
ar[0] = 42
console.log(ar[0], ar())

// frozen closure — indexed and named writes both throw
const fz: any = function fz() {}
Object.freeze(fz)
try { fz[6] = "x" } catch (e: any) { console.log("caught", e.constructor.name) }
console.log(fz[6])
try { fz.a = 1 } catch (e: any) { console.log("caught", e.constructor.name) }
console.log(fz.a)

// sealed closure — existing key writable, new key refused
const sl: any = function sl() {}
sl.a = 1
Object.seal(sl)
sl.a = 2
console.log(sl.a)
try { sl.b = 3 } catch (e: any) { console.log("caught", e.constructor.name) }
console.log(sl.b)

// preventExtensions closure — same new-key refusal
const pe: any = () => 1
pe.x = 1
Object.preventExtensions(pe)
pe.x = 5
console.log(pe.x)
try { pe.y = 6 } catch (e: any) { console.log("caught", e.constructor.name) }
console.log(pe.y)

// frozen promise expando
const p: any = Promise.resolve(1)
p.foo = "pre"
Object.freeze(p)
try { p.foo = "post" } catch (e: any) { console.log("caught", e.constructor.name) }
console.log(p.foo)
