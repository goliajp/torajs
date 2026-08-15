// 406-01 — a class expression inside an EXPRESSION-BODIED arrow must
// see the arrow's parameters. The parser drains the class synth into
// the arrow body (watermark, same protocol as the parse_stmt wrapper),
// so the body converges on the block-bodied shape the nested-class
// machinery already handles.

// paren form, class captures the param
const mk = (n: any) => class { m() { return n } }
console.log(new (mk(6))().m())

// bare single-param form
const mk2 = x => class { g() { return x + 1 } }
console.log(new (mk2(10))().g())

// capture-free expression body — the hoist lane lifts it to top level
const mkf = () => class { s() { return 7 } }
console.log(new (mkf())().s())

// nested expression-bodied arrows — each body adopts only its own synth
const outer = (a: any) => ((b: any) => class { sum() { return a + b } })
console.log(new (outer(3)(4))().sum())

// expression bodies inside both arms of a ternary
const pick = true ? (n: any) => class { v() { return n * 10 } } : (n: any) => class { v() { return n * 100 } }
console.log(new (pick(5))().v())

// async expression-bodied arrow returning a class
const mka = async (n: any) => class { m() { return n } }
mka(42).then((C: any) => console.log(new C().m()))
