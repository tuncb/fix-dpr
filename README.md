# fixdpr

CLI tool that updates Delphi `.dpr` program files.

It now supports four modes:

- `add-dependency`: existing behavior. Add a given new unit to `.dpr` files that require it.
- `insert-dependency`: insert a given new unit into selected `.dpr` files whether or not they currently depend on it, and optionally add that new unit's transitive dependency chain.
- `delete-dependency`: remove a given unit from selected `.dpr` files and also remove transitive dependencies that are no longer required by any remaining `.dpr` entry.
- `fix-dpr`: new behavior. Repair one target `.dpr` by traversing dependency chains from its existing `uses` entries and adding missing units found in the scanned search-path unit cache.

## Usage

```powershell
fixdpr add-dependency NEW_DEPENDENCY --search-path PATH [--search-path PATH] [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--ignore-dpr GLOB] [--disable-introduced-dependencies] [--fix-updated-dprs] [--show-infos] [--show-warnings]
```

```powershell
fixdpr insert-dependency NEW_DEPENDENCY --search-path PATH [--search-path PATH] (--target-path PATH | --target-dpr DPR_FILE) [--target-path PATH] [--target-dpr DPR_FILE] [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--ignore-dpr GLOB] [--disable-introduced-dependencies] [--show-infos] [--show-warnings]
```

```powershell
fixdpr delete-dependency OLD_DEPENDENCY --search-path PATH [--search-path PATH] (--target-path PATH | --target-dpr DPR_FILE) [--target-path PATH] [--target-dpr DPR_FILE] [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--ignore-dpr GLOB] [--show-infos] [--show-warnings]
```

```powershell
fixdpr fix-dpr DPR_FILE --search-path PATH [--search-path PATH] [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--show-infos] [--show-warnings]
```

## Arguments

### Shared arguments

- `--search-path PATH`: Required. Root directory to recursively scan for `.dpr` and `.pas`; can be repeated. Relative paths are resolved from the current working directory.
- `--ignore-path PATH`: Optional directory to skip recursively; can be repeated. Relative paths are resolved from the current working directory.
- `--show-infos`: Show detailed info messages.
- `--show-warnings`: Show detailed warning messages.
- `--delphi-path PATH`: Optional fallback source root for Delphi/VCL units; can be repeated. Units in these roots are used only for dependency resolution fallback and are not scanned for `.dpr` updates.
- `--delphi-version VERSION`: Optional Delphi/BDS version to resolve from Windows registry and use `<BDS Root>\source` as fallback roots; can be repeated. Accepts both `22.0` and `22` forms.

### `add-dependency` arguments

- `NEW_DEPENDENCY`: A `.pas` file path (absolute or relative to the current working directory).
- `--ignore-dpr GLOB`: Optional `.dpr` glob pattern to ignore; can be repeated. Relative patterns are resolved from the current working directory, then matched against absolute `.dpr` paths.
- `--disable-introduced-dependencies`: Disable inserting transitive dependencies referenced by `NEW_DEPENDENCY`; by default, these introduced dependencies are also inserted when needed.
- `--fix-updated-dprs`: After `add-dependency` updates files, run `fix-dpr` behavior on each updated `.dpr` to add additional missing dependencies from the search-path unit cache.

### `insert-dependency` arguments

- `NEW_DEPENDENCY`: A `.pas` file path (absolute or relative to the current working directory).
- `--target-path PATH`: Directory whose `.dpr` files should be updated recursively; can be repeated. Each target path must sit under one of the `--search-path` roots.
- `--target-dpr DPR_FILE`: Exact `.dpr` file to update; can be repeated. Each target `.dpr` must sit under one of the `--search-path` roots.
- `--ignore-dpr GLOB`: Optional `.dpr` glob pattern to ignore; can be repeated. Relative patterns are resolved from the current working directory, then matched against absolute `.dpr` paths.
- `--disable-introduced-dependencies`: Disable inserting transitive dependencies referenced by `NEW_DEPENDENCY`; by default, these introduced dependencies are also inserted after the root dependency.

### `fix-dpr` arguments

- `DPR_FILE`: Target `.dpr` file to repair (absolute or relative to the current working directory).

### `delete-dependency` arguments

- `OLD_DEPENDENCY`: A `.pas` file path (absolute or relative to the current working directory).
- `--target-path PATH`: Directory whose `.dpr` files should be updated recursively; can be repeated. Each target path must sit under one of the `--search-path` roots.
- `--target-dpr DPR_FILE`: Exact `.dpr` file to update; can be repeated. Each target `.dpr` must sit under one of the `--search-path` roots.
- `--ignore-dpr GLOB`: Optional `.dpr` glob pattern to ignore; can be repeated. Relative patterns are resolved from the current working directory, then matched against absolute `.dpr` paths.

## Examples

Add a new dependency for all matching `.dpr` files:

```powershell
fixdpr add-dependency .\repo\common\NewUnit.pas --search-path .\repo
```

Add dependency using Delphi fallback roots:

```powershell
fixdpr add-dependency `
  C:\RADStudio\source\rtl\common\System.Net.HttpClient.pas `
  --search-path .\repo `
  --delphi-path C:\RADStudio\source\rtl\common `
  --delphi-path C:\RADStudio\source\vcl
```

Insert dependency into all application `.dpr` files under one subtree:

```powershell
fixdpr insert-dependency `
  .\repo\common\NewUnit.pas `
  --search-path .\repo `
  --target-path .\repo\apps
```

Insert dependency into one explicit `.dpr` file:

```powershell
fixdpr insert-dependency `
  .\repo\common\NewUnit.pas `
  --search-path .\repo `
  --target-dpr .\repo\special\LegacyApp.dpr `
  --disable-introduced-dependencies
```

Delete dependency from selected `.dpr` files and cascade orphaned removals:

```powershell
fixdpr delete-dependency `
  .\repo\common\LegacyUnit.pas `
  --search-path .\repo `
  --target-path .\repo\apps
```

Delete dependency from one explicit `.dpr` file:

```powershell
fixdpr delete-dependency `
  .\repo\common\LegacyUnit.pas `
  --search-path .\repo `
  --target-dpr .\repo\apps\App1\App1.dpr
```

Add dependency and then run fix pass on all updated `.dpr` files:

```powershell
fixdpr add-dependency `
  .\repo\common\NewUnit.pas `
  --search-path .\repo `
  --fix-updated-dprs
```

Repair one `.dpr` by adding missing dependencies from search-path units:

```powershell
fixdpr fix-dpr `
  .\repo\app1\App1.dpr `
  --search-path .\repo
```

Repair one `.dpr` while honoring ignored paths:

```powershell
fixdpr fix-dpr `
  .\repo\app1\App1.dpr `
  --search-path .\repo `
  --delphi-path C:\RADStudio\source\rtl\common `
  --ignore-path .\repo\ignored
```

## Features

- `uses` lists can include `{$I ...}` / `{$INCLUDE ...}` fragments in both `.pas` and `.dpr` files. Include paths are resolved relative to the file that references them.
