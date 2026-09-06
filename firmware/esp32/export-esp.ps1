# Dot-source this file before building or profiling the ESP32 firmware.
# espup installs this toolchain below the current user's Rust toolchain directory.
$espToolchain = Join-Path $env:USERPROFILE ".rustup\toolchains\esp"
$espClangBin = Join-Path $espToolchain "xtensa-esp32-elf-clang\esp-clang\bin"
$espGccBin = Join-Path $espToolchain "xtensa-esp-elf\bin"

$Env:LIBCLANG_PATH = Join-Path $espClangBin "libclang.dll"
$Env:PATH = "$espClangBin;$espGccBin;" + $Env:PATH
