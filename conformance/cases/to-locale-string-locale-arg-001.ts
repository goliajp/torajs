// toLocaleString's locales / options arguments. The runtime formats
// en-US only, so a locale or option it cannot serve is a RangeError
// rather than an en-US answer; served requests format as usual.

function probe(label: string, f: () => string): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "throws", e instanceof RangeError ? "RangeError" : e instanceof TypeError ? "TypeError" : "other");
  }
}

const n: number = 1234.5;
const i: number = 1234567;
const big: bigint = 12345678901234567890n;
const d: Date = new Date(2020, 0, 2, 3, 4, 5);
const bad: Date = new Date(NaN);
const xs: number[] = [1234.5, 2];
const ss: string[] = ["a", "b"];
const empty: number[] = [];

// served: no argument, undefined, en / en-US in any case
probe("num bare", () => n.toLocaleString());
probe("num en-US", () => n.toLocaleString("en-US"));
probe("num en", () => i.toLocaleString("en"));
probe("num EN-us", () => n.toLocaleString("EN-us"));
probe("num undefined", () => n.toLocaleString(undefined));
probe("num list", () => n.toLocaleString(["en-US", "de-DE"]));
probe("num empty list", () => n.toLocaleString([]));
probe("num empty options", () => n.toLocaleString("en-US", {}));
probe("num unread option", () => n.toLocaleString("en-US", { timeZone: "UTC" } as any));
probe("big en-US", () => big.toLocaleString("en-US"));
probe("date en-US", () => d.toLocaleDateString("en-US"));
probe("arr en-US", () => xs.toLocaleString("en-US"));
probe("str arr de-DE", () => ss.toLocaleString("de-DE"));
probe("empty arr de-DE", () => JSON.stringify(empty.toLocaleString("de-DE")));
probe("invalid date de-DE", () => bad.toLocaleString("de-DE"));

// refused: another locale, an option, a malformed tag, null
probe("num de-DE", () => n.toLocaleString("de-DE"));
probe("num list de first", () => n.toLocaleString(["de-DE", "en-US"]));
probe("num option", () => n.toLocaleString("en-US", { minimumFractionDigits: 2 }));
probe("num malformed", () => n.toLocaleString("x_y!"));
probe("num null", () => n.toLocaleString(null as any));
probe("big de-DE", () => big.toLocaleString("de-DE"));
probe("date de-DE", () => d.toLocaleString("de-DE"));
probe("date option", () => d.toLocaleTimeString("en-US", { hour12: false }));
probe("arr de-DE", () => xs.toLocaleString("de-DE"));

// the any lane answers the same
const a: any = 1234.5;
const ad: any = d;
const aa: any = xs;
probe("any num en-US", () => a.toLocaleString("en-US"));
probe("any num de-DE", () => a.toLocaleString("de-DE"));
probe("any num option", () => a.toLocaleString(undefined, { style: "percent" }));
probe("any date de-DE", () => ad.toLocaleDateString("de-DE"));
probe("any arr de-DE", () => aa.toLocaleString("de-DE"));
probe("typed array de-DE", () => (new Float64Array([1.5]) as any).toLocaleString("de-DE"));

// the options are read before an unsupported locale is refused, each
// option getter once; a primitive options value reads as no options
let reads = 0;
const counted = { get minimumFractionDigits() { reads++; return undefined; } };
probe("getter once", () => n.toLocaleString("en-US", counted as any) + " " + reads);
probe("de with null options", () => n.toLocaleString("de", null as any));
probe("primitive options", () => n.toLocaleString("en-US", 5 as any));
probe("non-string locale element", () => n.toLocaleString([5] as any));
probe("call en-US", () => Number.prototype.toLocaleString.call(1234.5, "en-US"));
probe("call de", () => Number.prototype.toLocaleString.call(1234.5, "de"));
probe("wrapper en-US", () => new Number(1234.5).toLocaleString("en-US"));
