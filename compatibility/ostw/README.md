# OSTW reference oracle

This directory is evaluation-only infrastructure. The OSTW binary is downloaded
to `target/ostw-reference/`; it is neither committed nor a Cargo dependency.

`reference.json` pins the release tag, immutable tag commit, release asset size,
and SHA-256. `latest` is never an evidence identity.

```sh
python3 compatibility/ostw/run_oracle.py --acquire --ping
python3 compatibility/ostw/run_oracle.py --acquire --update
python3 compatibility/ostw/run_oracle.py
```

The runner drives `Deltinteger --langserver` using Content-Length framed JSON-RPC.
It opens a project workspace so `ds.toml` is visible, records diagnostics and the
custom `workshopCode` / `elementCount` notifications, and writes deterministic
JSON evidence. It never invokes the clipboard-bound default compiler path.
