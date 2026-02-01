# Implementation Checklist

## 1) Project setup + CLI
- [ ] Create crate layout: `src/main.rs`, `src/cli.rs`, `src/fs_walk.rs`, `src/parse/`, `src/graph/`, `src/dpr_edit.rs`, `src/report.rs`
- [ ] Add dependencies (clap, walkdir, rayon, anyhow/thiserror, hashbrown optional)
- [ ] Implement CLI args: `--search-path`, `--new-dependency`, `--ignore-paths`
- [ ] Wire logging and error handling

## 2) Filesystem scan + ignore handling
- [ ] Parse `--ignore-paths` (repeatable flag and/or comma-separated list)
- [ ] Normalize ignore entries (trim, normalize separators/case) and treat them as path prefixes
- [ ] Walk `--search-path` once, collecting `.pas` and `.dpr` while skipping ignored prefixes
- [ ] Use walkdir (or equivalent) with fast ignore checks to prune ignored directories early
- [ ] Normalize paths (case + separators) while preserving originals; canonicalize when safe

## 3) PAS unit index
- [ ] Implement lightweight lexer (skip `{}`, `(* *)`, `//`, strings)
- [ ] Parse `unit <Name>;` from each `.pas`
- [ ] Fallback to filename stem with warning when needed
- [ ] Build `unit_name -> UnitId`, `UnitId -> path, display_name`
- [ ] Detect and track ambiguous unit names

## 4) Dependency graph + closure
- [ ] Parse `uses` clauses in interface + implementation sections
- [ ] Build `deps` and reverse `rev` adjacency lists
- [ ] Compute dependents of `newUnit` via BFS/DFS into a bitset

## 5) DPR parse + modification
- [ ] Parse top-level `uses` list from each `.dpr`
- [ ] Skip if `newUnit` already present (case-insensitive)
- [ ] Insert new unit if any listed unit is in dependents
- [ ] Compute relative path to new `.pas` and match separator style
- [ ] Preserve line endings/indentation; atomic write

## 6) Reporting + exit codes
- [ ] Summarize counts: pas scanned, dpr scanned, dpr updated
- [ ] Collect warnings/errors and print at end
- [ ] Implement exit codes: 0 success, 1 warnings/failures, 2 invalid args

## 7) Tests
- [ ] Lexer tests (comments, strings, directives)
- [ ] PAS parsing tests (unit name + uses lists)
- [ ] DPR parsing/insertion tests (single-line, multi-line, `in 'path'`, comments)
- [ ] Integration tests for idempotency
- [ ] Optional perf test (synthetic repo)
