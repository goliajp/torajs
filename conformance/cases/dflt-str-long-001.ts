// S3.8 Str extension (knife B1): a runtime-undefined actual binds a
// typed param's long-string (>5-byte) literal default on the
// direct-call lane — interned static Str global through the
// or_default kernel's Heap tag (rc no-op via FLAG_STATIC_LITERAL).
// The boxed any-lane's long-string default is an open L3b (its
// synthesize stage has no string-pool channel).
function h(a: number, d: string = "longdefault"): string { return a + ":" + d }
let u: any = undefined
console.log(h(1, u))          // direct lane, runtime undefined
console.log(h(2))             // missing arg (call-site pad lane)
console.log(h(3, "explicit")) // explicit value stays
let us: any = "real"
console.log(h(4, us))         // any holding a real string
console.log(h(5, undefined))  // literal undefined (call-site pad lane)
function mix(a: string = "default-a", b: number = 42, c: string = "c"): string {
  return a + "|" + b + "|" + c
}
console.log(mix(u, u, u))     // long-str + num + short-str defaults together
console.log(mix())            // all missing
function uni(a: number, d: string = "日本語です"): string { return a + ":" + d }
console.log(uni(6, u))        // multibyte UTF-8 past the ShortStr cap
