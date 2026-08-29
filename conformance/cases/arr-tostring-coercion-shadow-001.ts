// Turning an array into a string is a WALK — §7.1.17 resolves
// `toString` on the receiver and §23.1.3.36 then resolves `join` — and
// the typed tier folds it to the join kernel, which can see neither.
// A method call stands down when the module might have touched those
// names (the checker types the callee `any`); a COERCION has no callee
// to type, so `String(xs)`, `xs + ""`, a template substitution and
// `Number(xs)` kept answering the kernel while `xs.toString()` right
// next to them answered the patch.
//
// A module that never names a builtin prototype is unaffected: the
// shadow set is empty and the fold stands.
const anchor: any = Object
const t: number[] = [3, 4]
const s: string[] = ["a", "b"]

console.log("pre String :", String(t))
console.log("pre plus   :", t + "")
console.log("pre tmpl   :", `${t}`)
console.log("pre Number :", Number(t))

// A `toString` patch has to reach every one of those spellings.
;(Array.prototype as any).toString = function () { return "9" }
console.log("ts  String :", String(t))
console.log("ts  plus   :", t + "")
console.log("ts  tmpl   :", `${t}`)
console.log("ts  Number :", Number(t))
console.log("ts  strarr :", String(s))
console.log("ts  method :", t.toString())
const keyed: any = { "9": "hit", "3,4": "miss" }
console.log("ts  key    :", keyed[t as any])
console.log("ts  eq     :", (t as any) == "9")
console.log("ts  concat :", "x".concat(t as any))
console.log("ts  nested :", [t as any].join("|"))
console.log("ts  json   :", JSON.stringify(t))

// So does a `join` patch, because §23.1.3.36 step 2 resolves it.
delete (Array.prototype as any).toString
;(Array.prototype as any).join = function () { return "J" }
console.log("jn  String :", String(t))
console.log("jn  plus   :", t + "")
console.log("jn  method :", t.toString())
console.log("jn  join   :", t.join("-"))

// And with both gone the walk reaches %Object.prototype%, whose badge
// is what every one of these spellings then answers.
delete (Array.prototype as any).join
console.log("gone String:", String(t))
console.log("gone plus  :", t + "")
console.log("gone tmpl  :", `${t}`)
console.log("gone method:", t.toString())
