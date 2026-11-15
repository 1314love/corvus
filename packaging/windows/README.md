# NSSM for the Windows installer

`nssm.exe` is NSSM 2.24-101-g897c7ad 64-bit (2017-04-26), vendored so the
Windows installer never downloads or resolves a service manager from PATH,
WinGet, Chocolatey, or another external location.

- Source: https://nssm.cc/
- License: public domain (NSSM is released into the public domain)
- SHA-256: `eee9c44c29c2be011f1f1e43bb8c3fca888cb81053022ec5a0060035de16d848`

`scripts/package.sh` copies this file to `dist/nssm.exe` and includes it in the
release zip. `scripts/install-windows.ps1` copies it to `<HomeDir>\bin\nssm.exe`
and uses only that copy for service operations.
