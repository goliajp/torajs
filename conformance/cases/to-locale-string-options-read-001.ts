// toLocaleString's locales / options follow the ECMA-402 constructors:
// every structurally valid language tag is accepted, a malformed one is
// a RangeError; every option the constructor reads is read once, in its
// order, and an invalid value is a RangeError / TypeError. Only en-US
// requests appear here, so the answers match any full Intl.

function probe(label: string, f: () => string): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "throws", e instanceof RangeError ? "RangeError" : e instanceof TypeError ? "TypeError" : "other");
  }
}

const n: number = 1234.5;
const big: bigint = 1234567n;
const d: Date = new Date(2020, 0, 2, 3, 4, 5);
const bad: Date = new Date(NaN);

// accepted tags: case, script, extensions, private use
probe("en-US", () => n.toLocaleString("en-US"));
probe("EN-us", () => n.toLocaleString("EN-us"));
probe("en-Latn-US", () => n.toLocaleString("en-Latn-US"));
probe("en-US-u-nu-latn", () => n.toLocaleString("en-US-u-nu-latn"));
probe("en-US-x-priv", () => n.toLocaleString("en-US-x-priv"));
probe("list", () => n.toLocaleString(["en-US", "en"]));
probe("big en-US", () => big.toLocaleString("en-US"));
probe("date en-US", () => d.toLocaleDateString("en-US"));

// malformed tags: a RangeError, whether literal or computed
const tags: string[] = ["", "en_US", "i-klingon", "en-u", "de-gregory-gregory", "x-priv", "root", "abcdefghi"];
for (const t of tags) {
  probe("tag " + JSON.stringify(t), () => n.toLocaleString(t));
}
probe("literal en_US", () => n.toLocaleString("en_US"));
probe("literal i", () => n.toLocaleString("i"));
probe("big literal en_US", () => big.toLocaleString("en_US"));
probe("date literal en_US", () => d.toLocaleString("en_US"));
probe("nan date en_US", () => bad.toLocaleString("en_US"));
probe("list with bad", () => n.toLocaleString(["en-US", "x_y"]));
probe("null", () => n.toLocaleString(null as any));
probe("bad element", () => n.toLocaleString([5] as any));
probe("NaN element", () => n.toLocaleString([NaN] as any));

// option values
probe("localeMatcher null", () => n.toLocaleString("en-US", { localeMatcher: null } as any));
probe("style invalid", () => n.toLocaleString("en-US", { style: "invalid" } as any));
probe("style currency no code", () => n.toLocaleString("en-US", { style: "currency" }));
probe("currency bad code", () => n.toLocaleString("en-US", { style: "currency", currency: "ßP" }));
probe("msd -Infinity", () => n.toLocaleString("en-US", { maximumSignificantDigits: -Infinity }));
probe("mnfd 101", () => n.toLocaleString("en-US", { minimumFractionDigits: 101 }));
probe("mnfd > mxfd", () => n.toLocaleString("en-US", { minimumFractionDigits: 3, maximumFractionDigits: 1 }));
probe("unit bad", () => n.toLocaleString("en-US", { style: "unit", unit: "meters" }));
probe("useGrouping bad", () => n.toLocaleString("en-US", { useGrouping: "sometimes" } as any));
probe("big style invalid", () => big.toLocaleString("en-US", { style: "invalid" } as any));
probe("date weekday bad", () => d.toLocaleString("en-US", { weekday: "tiny" } as any));
probe("date timeZone bad", () => d.toLocaleString("en-US", { timeZone: "+25:00" }));
probe("date style + component", () => d.toLocaleString("en-US", { dateStyle: "short", year: "numeric" }));
probe("dateString timeStyle", () => d.toLocaleDateString("en-US", { timeStyle: "short" }));
probe("timeString dateStyle", () => d.toLocaleTimeString("en-US", { dateStyle: "short" }));
probe("nan date bad option", () => bad.toLocaleString("en-US", { weekday: "tiny" } as any));

// each option read once, in the constructor's order
const log: string[] = [];
function spy(names: string[]): any {
  const o: any = {};
  for (const k of names) {
    Object.defineProperty(o, k, { get: () => { log.push(k); return undefined; } });
  }
  return o;
}
const numberNames: string[] = ["signDisplay", "useGrouping", "compactDisplay", "trailingZeroDisplay", "roundingPriority", "roundingMode", "roundingIncrement", "maximumSignificantDigits", "minimumSignificantDigits", "maximumFractionDigits", "minimumFractionDigits", "minimumIntegerDigits", "notation", "unitDisplay", "unit", "currencySign", "currencyDisplay", "currency", "style", "numberingSystem", "localeMatcher"];
probe("number order", () => n.toLocaleString("en-US", spy(numberNames)) + " " + log.join(","));
log.length = 0;
probe("bigint order", () => big.toLocaleString(undefined, spy(numberNames)) + " " + log.length);
const dateNames: string[] = ["timeStyle", "dateStyle", "formatMatcher", "timeZoneName", "fractionalSecondDigits", "second", "minute", "hour", "dayPeriod", "day", "month", "year", "era", "weekday", "timeZone", "hourCycle", "hour12", "numberingSystem", "calendar", "localeMatcher"];
log.length = 0;
probe("date order", () => d.toLocaleDateString("en-US", spy(dateNames)).length + " " + log.join(","));

// the any lane answers the same
const a: any = n;
const ad: any = d;
probe("any en-Latn-US", () => a.toLocaleString("en-Latn-US"));
probe("any en_US", () => a.toLocaleString("en_US"));
probe("any style invalid", () => a.toLocaleString("en-US", { style: "invalid" }));
probe("any date timeStyle", () => ad.toLocaleDateString("en-US", { timeStyle: "short" }));
probe("call en_US", () => Number.prototype.toLocaleString.call(1234.5, "en_US"));
