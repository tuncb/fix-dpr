# fixdpr

CLI tool that scans a Delphi project tree and updates `.dpr` program files to
add a missing unit dependency. It finds all `.pas` units, builds a dependency
graph from their `uses` clauses, then inserts the new unit into any `.dpr` uses
list that already references a unit that depends on it.

## Usage

```
fixdpr --search-path PATH [--search-path PATH] --new-dependency VALUE [--delphi-path PATH] [--delphi-version VERSION] [--ignore-path PATH] [--ignore-dpr GLOB]
```

### Arguments

- `--search-path PATH`: Root directory to recursively scan for `.dpr` and `.pas`;
  can be repeated. Relative paths are resolved from the current working
  directory.
- `--new-dependency VALUE`: A `.pas` file path (absolute or relative to the
  current working directory).
- `--delphi-path PATH`: Optional fallback source root for Delphi/VCL units;
  can be repeated. Units in these roots are used only for dependency
  resolution fallback and are not scanned for `.dpr` updates.
- `--delphi-version VERSION`: Optional Delphi/BDS version to resolve from
  Windows registry and use `<BDS Root>\source` as fallback roots; can be
  repeated. Accepts both `22.0` and `22` forms.
- `--ignore-path PATH`: Optional directory to skip recursively; can be repeated.
  Relative paths are resolved from the current working directory.
- `--ignore-dpr GLOB`: Optional `.dpr` glob pattern to ignore; can be repeated.
  Relative patterns are resolved from the current working directory, then matched
  against absolute `.dpr` paths.
- `--show-infos`: Show detailed info messages.
- `--show-warnings`: Show detailed warning messages.

## Examples

Scan a single root:

```powershell
fixdpr --search-path .\repo --new-dependency .\repo\common\NewUnit.pas
```

Scan multiple roots:

```powershell
fixdpr `
  --search-path .\repo\app1 `
  --search-path .\repo\app2 `
  --new-dependency .\repo\common\NewUnit.pas
```

Use Delphi source roots as fallback unit resolution:

```powershell
fixdpr `
  --search-path .\repo `
  --new-dependency C:\RADStudio\source\rtl\common\System.Net.HttpClient.pas `
  --delphi-path C:\RADStudio\source\rtl\common `
  --delphi-path C:\RADStudio\source\vcl
```

Resolve fallback roots directly from Delphi version:

```powershell
fixdpr `
  --search-path .\repo `
  --new-dependency C:\Program Files (x86)\Embarcadero\Studio\22.0\source\rtl\common\System.Net.HttpClient.pas `
  --delphi-version 22.0
```

Ignore one or more directories recursively:

```powershell
fixdpr `
  --search-path .\repo `
  --new-dependency .\repo\common\NewUnit.pas `
  --ignore-path .\repo\ignored `
  --ignore-path C:\external\legacy
```

Ignore specific `.dpr` files with glob patterns:

```powershell
fixdpr `
  --search-path .\repo `
  --new-dependency .\repo\common\NewUnit.pas `
  --ignore-dpr ".\repo\app4\*.dpr" `
  --ignore-dpr "C:\work\repo\legacy\**\*.dpr"
```

## Features

- `uses` lists can include `{$I ...}` / `{$INCLUDE ...}` fragments in both `.pas`
and `.dpr` files. Include paths are resolved relative to the file that references
them.
