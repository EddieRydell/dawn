# Dawn Language Server for LSP4IJ

This template starts the local Dawn language server from this repository.

Server command on Windows:

```text
cmd /c C:\Users\eddie\dawn\scripts\dawn-lsp.cmd
```

File mapping:

```text
*.dawn -> dawn
```

The launcher builds `dawn-lsp` if `target\debug\dawn-lsp.exe` does not exist, then starts the server over stdio.
