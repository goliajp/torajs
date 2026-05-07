// T-19.n (v0.5.0) — closure-cb variants of `.catch` / `.finally`.
// Mirror the T-15.g.5 .then closure dispatcher: env+8 holds
// fn_addr, runtime calls `(env*, reason: i64) -> i64` for catch
// and `(env*) -> void` for finally. Selection between simple and
// closure variants happens at the call site based on cb's static
// type (Type::Closure → env-pointer dispatcher).
//
// Without these dispatchers, a capturing-closure cb would have
// the env pointer treated as a raw fn pointer and the runtime
// would jump into env+0 (the universal heap header) → SEGV.

let recovery = 999
let recover = function(reason: number): number { return reason + recovery }

let p1 = Promise.reject(7).catch(recover)
console.log(await p1)                         // 1006

let p2 = Promise.resolve(5).catch(recover)
console.log(await p2)                         // 5 (cb NOT called)

let log_count = 0
let bump = function(): void { log_count = log_count + 1 }

let p3 = Promise.resolve(42).finally(bump)
console.log(await p3)                         // 42

let p4 = Promise.reject(11).finally(bump).catch(recover)
console.log(await p4)                         // 1010 (11 + 999)

console.log(log_count)                        // 2 — fired on both
