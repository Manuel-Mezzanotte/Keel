# Licence texts the packages do not carry

`scripts/third-party-licenses.py` reads each package's own licence files.
This folder holds what it cannot find there.

- `<package>/*.txt`: texts for a package, added to its own and kept whole.
  `v8/` has those of what comes compiled inside the `v8` crate: its prebuilt
  library holds V8 and the libraries V8 is built with, whose licence files the
  crate leaves out. Each file starts with what it covers and where the text was
  taken from, on 23 November 2026.
- `spdx/MIT.txt`: the MIT licence, for the packages that declare it but carry
  no file; the script writes in the authors the package lists, and says so.

When the script stops on a package with no licence text, put the text its
authors publish in `<package>/`, with the same first two lines.
