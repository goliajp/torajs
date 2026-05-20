export type Conformance = {
  pass: number
  fail: number
  skip: number
  raw: string
}

// tc39/test262 spec-conformance snapshot. Populated from
// `hardev/test262-latest.json` (produced by `torajs-test262 --json`).
// Absent when no full run has been recorded yet — dashboard renders an
// explicit "not yet measured" stub rather than fabricating numbers.
export type Test262 = {
  ranAt: string
  headSha: string
  elapsedSec: number
  workers: number
  limit: number | null
  totalCases: number
  ran: number
  pass: number
  bug: number
  incompatible: number
  bunSkip: number
  harnessError: number
  inScope: number
  trAccepted: number
  passRateInScope: number
  passRateTrAccepted: number
}

export type Headline = {
  value: string
  unit: string
  label: string
  sub: string
}

export type Pillar = {
  key: string
  name: string
  tagline: string
  scope: string
  metricName: string
  status: string
}

export type Release = {
  version: string
  date: string
  title: string
  bullets: string[]
}

export type BenchRow = {
  case: string
  bunAot: number | null
  bunJsc: number | null
  nodeV8: number | null
  torajs: number
  speedup: number | null
}

export type Bench = {
  rows: BenchRow[]
  geomeanBunAot: number | null
  geomeanNodeV8: number | null
  peak: number | null
  peakCase: string | null
  min: number | null
  minCase: string | null
  caseCount: number
  wonCount: number
  sourceFile: string
  gitSha: string
  startedAt: string
  host: string
}

export type Commit = {
  hash: string
  subject: string
}

export type RoadmapStatus = 'DONE' | 'CURRENT' | 'queued' | 'post-v1.0'

export type RoadmapPhase = {
  id: string
  title: string
  status: RoadmapStatus
}

export type Torajs = {
  commit: string
  commitSubject: string
  phase: string | null
  phaseTitle: string | null
  conformance: Conformance
}

export type Rotation = {
  rotationId: string
  at: string
  ts: number
  trigger: 'self' | 'manual' | 'hook' | 'daemon'
  prevHead: string
  handoffSha: string
  handoffAgeSec: number
  conformanceBefore: string | null
  commitsInSession: number | null
}

// autorun pillar (5th hardev pillar — rotation governance). P0+P0.1
// shipped: manual trigger.sh + JSONL log; P1 (Stop hook + watcher +
// auto-/clear + auto-resume) unlocks once baseline ≥ baselineTarget
// rotations are accumulated. Null when no rows in rotations.jsonl yet.
export type Autorun = {
  total: number
  baselineTarget: number
  bySelf: number
  byManual: number
  byOther: number
  last: Rotation | null
  recent: Rotation[]
  rotationsFile: string
}

export type SnapshotData = {
  generatedAt: string
  hardevVersion: string
  conformance: Conformance
  test262: Test262 | null
  torajs: Torajs
  roadmap: RoadmapPhase[]
  headline: Headline[]
  pillars: Pillar[]
  changelog: Release[]
  bench: Bench
  commits: Commit[]
  headSha: string | null
  autorun: Autorun | null
}
