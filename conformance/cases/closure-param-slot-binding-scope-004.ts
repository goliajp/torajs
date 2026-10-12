// a parameter default reading a binding initialized from a field read
// hands the closure cell to the fn-typed parameter it fills
type F = (x: number) => number;

function m(f: F): number { return f(1) }
function m2(f: F): number { return f(2) }
function m3(f: F): number { return f(3) }
function twice(x: number): number { return x * 2 }
let k = 7;

function viaDestr({ g }: { g: F }) { function t(a: F = g) { return m(a) } return t() }
function viaMember(p: { g: F }) { const g = p.g; function t(b: F = g) { return m2(b) } return t() }
const viaArrow = ({ g }: { g: F }) => ((c: F = g) => m3(c))();

console.log(viaDestr({ g: (x) => x + k }), viaDestr({ g: twice }));
console.log(viaMember({ g: (x) => x * k }), viaMember({ g: twice }));
console.log(viaArrow({ g: (x) => x - k }), viaArrow({ g: twice }));
