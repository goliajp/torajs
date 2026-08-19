// Injected-builtin params must not shadow user names — the synthesized
// Error classes' ctor/method params (message / options / x / errors)
// used to flatten into __cm_/__sm_ FnDecl params carrying those bare
// names, and every by-name census (the fnexpr-this shadow gate among
// them) then saw a user binding of the same name as shadowed and
// refused promotion. The injected params now spell __bi_<name>.
var x = function () { return this; };
console.log(typeof x());

var message = function () { return this; };
console.log(message() === undefined);

var options = function () { this.probe = 1; };
var o: any = new options();
console.log(o.probe);

var errors = function () { return this; };
console.log(errors() === undefined);

// the renamed params stay invisible: the Error faces still work
var e: any = new RangeError("r", { cause: "c" });
console.log(e.message, e.cause, Error.isError(e));
