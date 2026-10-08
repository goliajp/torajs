// A function value called through `any` binds each argument as
// passed, and a binding its body stores the parameter into holds that
// value as it is: the typed parameter does not convert it.

let n = 0
const f: any = (x: number) => {
  n = x
}
f("q")
console.log(n, typeof n)
f(4)
console.log(n + 1)

let s: string = ""
const g: any = (v: string) => {
  s = v
}
g(5)
console.log(s, typeof s)

let flag: boolean = false
const h: any = (b: boolean) => {
  flag = b
}
h("yes")
console.log(flag, typeof flag)

let total = 0
const add: any = (v: number) => {
  total = total + v
}
add(1)
add("2")
console.log(total)

let last = 0
function keep(v: number) {
  last = v
}
const k: any = keep
k("named")
console.log(last)
keep(3)
console.log(last * 2)

// a closure only ever called through its typed face keeps its number binding
let typed = 0
const t = (v: number) => {
  typed = typed + v
}
t(2)
t(3)
console.log(typed, typeof typed)
