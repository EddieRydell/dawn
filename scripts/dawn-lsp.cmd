@echo off
setlocal

set "ROOT=%~dp0.."
set "SERVER=%ROOT%\target\debug\dawn-lsp.exe"

if not exist "%SERVER%" (
  echo dawn-lsp.exe is missing. Run cargo build -p dawn-lsp from %ROOT% first. 1>&2
  exit /b 1
)

"%SERVER%"
