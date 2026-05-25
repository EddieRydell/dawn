$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$uiRoot = Join-Path $root "apps/desktop/src/ui"
$violations = @()

Get-ChildItem $uiRoot -Recurse -Filter "*.rs" |
    Where-Object { $_.FullName -notlike "*\theme.rs" } |
    ForEach-Object {
        $relative = Resolve-Path -Relative $_.FullName
        $isDropdownMenu = $_.FullName -like "*\dropdown_menu.rs"
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
            if ($_ -match "floem::menu::\{|floem::menu::Menu|floem::menu::MenuItem") {
                $violations += "${relative}:${lineNumber}: native Floem menu API; use ui::dropdown_menu"
            }
            if ($_ -match "\bMenu::new\b|\bMenuItem::new\b") {
                $violations += "${relative}:${lineNumber}: native Floem menu builder; use ui::dropdown_menu"
            }
            if ($_ -match "\.context_menu\s*\(|\.popout_menu\s*\(") {
                $violations += "${relative}:${lineNumber}: raw menu decorator; use ui::dropdown_menu"
            }
            if ($_ -match "\badd_overlay\s*\(|\bremove_overlay\s*\(") {
                $violations += "${relative}:${lineNumber}: direct overlay use; route command menus through ui::dropdown_menu"
            }
            if ($isDropdownMenu -and $_ -match "\bui_button\s*\(") {
                $violations += "${relative}:${lineNumber}: dropdown menu rows must not use ui_button"
            }
        }
    }

if ($violations.Count -gt 0) {
    $violations | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Host "UI theme check passed."
