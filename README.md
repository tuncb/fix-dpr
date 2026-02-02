# fixdpr

CLI tool that scans a Delphi project tree and updates `.dpr` program files to
add a missing unit dependency. It finds all `.pas` units, builds a dependency
graph from their `uses` clauses, then inserts the new unit into any `.dpr` uses
list that already references a unit that depends on it.

## Usage

```
fixdpr --search-path PATH --new-dependency VALUE [--ignore-paths PATHS]
```

### Arguments

- `--search-path PATH`: Root folder to recursively scan for `.dpr` and `.pas`.
- `--new-dependency VALUE`: A `.pas` file path (absolute or relative to the
  current working directory).
- `--ignore-paths PATHS`: Optional folder prefixes to skip; can be repeated or
  comma-separated.
