# fixdpr

CLI tool that scans a Delphi project tree and updates `.dpr` program files to
add a missing unit dependency. It finds all `.pas` units, builds a dependency
graph from their `uses` clauses, then inserts the new unit into any `.dpr` uses
list that already references a unit that depends on it.

## Usage

```
fixdpr --search-path GLOB [--search-path GLOB] --new-dependency VALUE [--ignore-paths PATHS] [--ignore-dpr GLOB]
```

### Arguments

- `--search-path GLOB`: Root directory pattern to recursively scan for `.dpr` and
  `.pas`; can be repeated. Relative patterns are resolved from the current
  working directory and matched against absolute directory paths. Non-directory
  matches are ignored. If at least one search root matches, unmatched patterns
  are reported as warnings.
- `--new-dependency VALUE`: A `.pas` file path (absolute or relative to the
  current working directory).
- `--ignore-paths PATHS`: Optional folder prefixes to skip; can be repeated or
  comma-separated.
- `--ignore-dpr GLOB`: Optional `.dpr` glob pattern to ignore; can be repeated.
  Relative patterns are resolved from the current working directory, then matched
  against absolute `.dpr` paths.
- `--show-infos`: Show detailed info messages.
- `--show-warnings`: Show detailed warning messages.


## Features

- `uses` lists can include `{$I ...}` / `{$INCLUDE ...}` fragments in both `.pas`
and `.dpr` files. Include paths are resolved relative to the file that references
them.
