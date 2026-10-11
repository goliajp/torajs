// A nested function declaration or arrow reading a top-level function's
// name that a scope in between rebinds calls that binding.
function k() { return 1 }
function param(k: () => number) { function h() { return k() } return h() }
function local() { const k = () => 2; function h() { return k() } return h() }
function forHead() { for (let k = () => 3, i = 0; i < 1; i++) { function h() { return k() } return h() } return 0 }
function caught() { try { throw () => 4 } catch (k) { function h() { return (k as any)() } return h() } }
const arrowParam = (k: () => number) => { function h() { return k() } return h() };
function localFn() { function k() { return 6 } { function h() { return k() } return h() } }
function localClass() { class k { static z() { return 7 } } function h() { return k.z() } return h() }
console.log(param(() => 9), local(), forHead(), caught(), arrowParam(() => 5), localFn(), localClass(), k());

function arrowForHead() { for (let k = () => 3, i = 0; i < 1; i++) { return (() => k())() } return 0 }
function arrowClass() { class k { static z() { return 7 } } return (() => k.z())() }
function arrowVar() { let r = 0; { var k = () => 8; } r = (() => k())(); return r }
function arrowLetAfter() { const f = () => k(); const k = () => 9; return f() }
class M { m() { const k = () => 10; return (() => k())() } }
console.log(arrowForHead(), arrowClass(), arrowVar(), arrowLetAfter(), new M().m(), k());
