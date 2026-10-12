// a numeric binding read out of a field in one function shares its
// name with a top-level function; that function returned through a
// fn-typed return, and passed to a fn-typed parameter, is a plain
// function wherever the numeric binding is out of scope
type F = (x: number) => number;
function inc(x: number): number { return x + 1 }
function counter(o: { inc: number }) { const inc = o.inc; return inc * 10 }
console.log(counter({ inc: 4 }));
function r(): F { return inc }
console.log(r()(5));
function m(f: F): number { return f(1) }
console.log(m(inc));
