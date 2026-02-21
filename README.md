# fixdpr

CLI tool that updates Delphi `.dpr` program files.

It now supports two modes:

- `add-dependency`: existing behavior. Add a given new unit to `.dpr` files that require it.
- `fix-dpr`: new behavior. Repair one target `.dpr` by traversing dependency chains from its existing `uses` entries and adding missing units found in the scanned search-path unit cache.

## Usage

```powershell
fixdpr add-dependency --search-path PATH [--search-path PATH] --new-dependency VALUE [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--ignore-dpr GLOB] [--disable-introduced-dependencies] [--fix-updated-dprs] [--show-infos] [--show-warnings]
```

```powershell
fixdpr fix-dpr --search-path PATH [--search-path PATH] --dpr-file FILE [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--ignore-dpr GLOB] [--show-infos] [--show-warnings]
```

## Arguments

### Common arguments

- `--search-path PATH`: Root directory to recursively scan for `.dpr` and `.pas`; can be repeated. Relative paths are resolved from the current working directory.
- `--ignore-path PATH`: Optional directory to skip recursively; can be repeated. Relative paths are resolved from the current working directory.
- `--ignore-dpr GLOB`: Optional `.dpr` glob pattern to ignore; can be repeated. Relative patterns are resolved from the current working directory, then matched against absolute `.dpr` paths.
- `--show-infos`: Show detailed info messages.
- `--show-warnings`: Show detailed warning messages.

### `add-dependency` arguments

- `--new-dependency VALUE`: A `.pas` file path (absolute or relative to the current working directory).
- `--delphi-path PATH`: Optional fallback source root for Delphi/VCL units; can be repeated. Units in these roots are used only for dependency resolution fallback and are not scanned for `.dpr` updates.
- `--delphi-version VERSION`: Optional Delphi/BDS version to resolve from Windows registry and use `<BDS Root>\source` as fallback roots; can be repeated. Accepts both `22.0` and `22` forms.
- `--disable-introduced-dependencies`: Disable inserting transitive dependencies referenced by `--new-dependency`; by default, these introduced dependencies are also inserted when needed.
- `--fix-updated-dprs`: After `add-dependency` updates files, run `fix-dpr` behavior on each updated `.dpr` to add additional missing dependencies from the search-path unit cache.

### `fix-dpr` arguments

- `--dpr-file FILE`: Target `.dpr` file to repair (absolute or relative to the current working directory).
- `--delphi-path PATH`: Optional fallback source root for Delphi/VCL units; can be repeated. Units in these roots are used only for dependency resolution fallback and are not scanned for `.dpr` updates.
- `--delphi-version VERSION`: Optional Delphi/BDS version to resolve from Windows registry and use `<BDS Root>\source` as fallback roots; can be repeated. Accepts both `22.0` and `22` forms.

## Examples

Add a new dependency for all matching `.dpr` files:

```powershell
fixdpr add-dependency --search-path .\repo --new-dependency .\repo\common\NewUnit.pas
```

Add dependency using Delphi fallback roots:

```powershell
fixdpr add-dependency `
  --search-path .\repo `
  --new-dependency C:\RADStudio\source\rtl\common\System.Net.HttpClient.pas `
  --delphi-path C:\RADStudio\source\rtl\common `
  --delphi-path C:\RADStudio\source\vcl
```

Add dependency and then run fix pass on all updated `.dpr` files:

```powershell
fixdpr add-dependency `
  --search-path .\repo `
  --new-dependency .\repo\common\NewUnit.pas `
  --fix-updated-dprs
```

Repair one `.dpr` by adding missing dependencies from search-path units:

```powershell
fixdpr fix-dpr `
  --search-path .\repo `
  --dpr-file .\repo\app1\App1.dpr
```

Repair one `.dpr` while honoring ignored paths/patterns:

```powershell
fixdpr fix-dpr `
  --search-path .\repo `
  --delphi-path C:\RADStudio\source\rtl\common `
  --dpr-file .\repo\app1\App1.dpr `
  --ignore-path .\repo\ignored `
  --ignore-dpr ".\repo\legacy\**\*.dpr"
```

## Features

- `uses` lists can include `{$I ...}` / `{$INCLUDE ...}` fragments in both `.pas` and `.dpr` files. Include paths are resolved relative to the file that references them.
