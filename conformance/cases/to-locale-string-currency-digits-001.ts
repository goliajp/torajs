// CurrencyDigits (ECMA-402 §6.3.2) sets the currency style's default
// fraction digits, which roundingIncrement and an explicit minimum are
// checked against
function attempt(label: string, opts: any): void {
  try {
    (1).toLocaleString("en-US", opts);
    console.log(label, "ok");
  } catch (e) {
    console.log(label, e instanceof RangeError ? "RangeError" : "other");
  }
}
attempt("jpy-increment", { style: "currency", currency: "JPY", roundingIncrement: 5, minimumFractionDigits: 1 });
attempt("jpy-lower", { style: "currency", currency: "jpy", roundingIncrement: 5, minimumFractionDigits: 1 });
attempt("usd-increment", { style: "currency", currency: "USD", roundingIncrement: 5, minimumFractionDigits: 1 });
attempt("bhd-max", { style: "currency", currency: "BHD", maximumFractionDigits: 2 });
attempt("usd-max", { style: "currency", currency: "USD", maximumFractionDigits: 1 });
attempt("clf-max", { style: "currency", currency: "CLF", maximumFractionDigits: 3 });
attempt("bhd-increment", { style: "currency", currency: "BHD", roundingIncrement: 5, minimumFractionDigits: 2 });
attempt("clf-increment", { style: "currency", currency: "CLF", roundingIncrement: 5, minimumFractionDigits: 4 });
const n = 5;
console.log(n.toLocaleString("en", {}, { extra: [1, 2, 3] }).length);
