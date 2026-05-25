$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$uiRoot = Join-Path $root "apps/desktop/src/ui"
$violations = @()

Get-ChildItem $uiRoot -Recurse -Filter "*.rs" |
    Where-Object { $_.FullName -notlike "*\theme.rs" } |
    ForEach-Object {
        $relative = Resolve-Path -Relative $_.FullName
        $lineNumber = 0
        Get-Content $_.FullName | ForEach-Object {
            $lineNumber += 1
            if ($_ -match "Color::") {
                $violations += "${relative}:${lineNumber}: raw Color constructor; use ui::theme tokens"
            }
            if ($_ -match '"#[0-9A-Fa-f]{6,8}"') {
                $violations += "${relative}:${lineNumber}: raw hex color; add a ui::theme token"
            }
            if ($_ -match "\.font_size\(\s*[0-9]+(\.[0-9]+)?\s*\)") {
                $violations += "${relative}:${lineNumber}: raw font size; use a ui::theme typography token"
            }
        }
    }

if ($violations.Count -gt 0) {
    $violations | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "UI theme check passed."
