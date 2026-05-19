export type Conformance = {
  pass: number
  fail: number
  skip: number
  raw: string
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

export type SnapshotData = {
  generatedAt: string
  hardevVersion: string
  conformance: Conformance
  headline: Headline[]
  pillars: Pillar[]
  changelog: Release[]
  bench: Bench
  commits: Commit[]
  headSha: string | null
}
