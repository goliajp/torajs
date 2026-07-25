# A/B runs

Full bench runs taken deliberately at a commit other than HEAD, to
compare against it. They live here rather than one level up because
the dashboard picks the newest `started_at` in `bench/results/` and
calls it the current state — an A/B run starts later than the HEAD run
it is being compared with, so leaving it there makes the dashboard
advertise an older commit's numbers.

Read them against the HEAD run of the same day, and read both against
an untouched control runtime: machine state drifts by several percent
within minutes, so two runs' `run_ms` are not directly comparable. The
comparable quantity is the tr/competitor ratio *within* one run. See
`.claude/rules/torajs-perf-decomposition.md`.
