// Date.prototype.toLocaleString / toLocaleDateString / toLocaleTimeString —
// en-US default locale, "M/D/YYYY, h:mm:ss AM/PM" format. Output is
// local-time so the fixture matches bun on whichever host TZ ships it.

const ms_per_min = 60 * 1000;
const ms_per_hour = 60 * ms_per_min;
const jst_offset_ms = 9 * ms_per_hour;

function row(label: string, ms: number): void {
  const d = new Date(ms);
  console.log(label, "|", d.toLocaleString(), "|", d.toLocaleDateString(), "|", d.toLocaleTimeString());
}

// epoch — JST 09:00 AM
row("epoch", 0);

// midnight local
row("mid", -jst_offset_ms);

// noon (12 PM boundary)
row("noon", 12 * ms_per_hour - jst_offset_ms);

// 1 PM (start of PM cycle after noon)
row("1pm", 13 * ms_per_hour - jst_offset_ms);

// 11 PM
row("11pm", 23 * ms_per_hour - jst_offset_ms);

// 11:59:59 PM (rollover - 1s)
row("11_59_59pm", 23 * ms_per_hour + 59 * ms_per_min + 59 * 1000 - jst_offset_ms);

// 12:01 AM (1 min past midnight)
row("12_01am", 1 * ms_per_min - jst_offset_ms);

// Dec 31 1999 11:59:59 PM (Y2K - 1s, local)
row("y2k_eve", 946684800000 - 1000 - jst_offset_ms);

// Y2K Jan 1 2000 00:00:00
row("y2k", 946684800000 - jst_offset_ms);

// Mar 5 2024 midnight (non-zero month/day)
row("mar5_2024", new Date(Date.UTC(2024, 2, 5, 0, 0, 0)).getTime() - jst_offset_ms);

// Oct 31 2024 3:30:07 PM
row("oct31_2024", new Date(Date.UTC(2024, 9, 31, 15, 30, 7)).getTime() - jst_offset_ms);

// Recent timestamp (2023-11-15)
row("late_2023", 1700000000000);
