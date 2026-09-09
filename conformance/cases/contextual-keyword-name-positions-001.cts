// The remaining BindingIdentifier positions. Each had its own match on
// `Token::Ident` and its own subset of the contextual keywords, so the
// answer depended on which position the word stood in rather than on
// what reserves it. Script goal; a module reserves `let` / `yield` /
// `await` everywhere and that face is an early error.
var fe1 = function let() { return 1; };
console.log(fe1());
var fe2 = function async() { return 2; };
console.log(fe2());
var fe3 = function type() { return 3; };
console.log(fe3());

class async { m() { return 4; } }
console.log(new async().m());
class type { m() { return 5; } }
console.log(new type().m());

function catchLet() { try { throw 6; } catch (let) { return let; } }
console.log(catchLet());
function catchYield() { try { throw 7; } catch (yield) { return yield; } }
console.log(catchYield());
function catchAsync() { try { throw 8; } catch (async) { return async; } }
console.log(catchAsync());
function catchType() { try { throw 9; } catch (type) { return type; } }
console.log(catchType());

for (var let of [10]) { console.log(let); }
for (var yield of [11]) { console.log(yield); }
for (var await of [12]) { console.log(await); }
for (var let in { a: 13 }) { console.log(let); }

var a1 = let => let + 1;
console.log(a1(13));
var a2 = yield => yield + 1;
console.log(a2(14));
var a3 = async => async + 1;
console.log(a3(15));
var a4 = type => type + 1;
console.log(a4(16));
var a5 = await => await + 1;
console.log(a5(17));

// §15.3 — `ArrowParameters [no LineTerminator here] =>`. A newline
// before the arrow is not an arrow.
var brk = 19;
var notArrow = brk
console.log(notArrow);
