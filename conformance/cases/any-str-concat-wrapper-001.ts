const wn: any = new Number(5)
const ws: any = new String("s")
const wb: any = new Boolean(false)
console.log("x" + wn, "x" + ws, "x" + wb, wn + "y", `${wn}`, String(ws))
Number.prototype.valueOf = function () { return 42 }
String.prototype.valueOf = function () { return "V" }
Boolean.prototype.valueOf = function () { return "B" }
console.log("x" + wn, "x" + ws, "x" + wb, wn + "y", `${wn}`, String(ws))
function f(mn: number | null) { let v = "a" + mn; v = v + 1; return v }
console.log(f(2), f(null))
