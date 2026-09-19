# OPY Compatibility Ownership

The OPY compatibility manifest, parser metadata, semantic catalog, and
reference probes are owned by `opy-rs`. Wright must not copy those data sets or
reimplement OPY name/member/enum resolution.

Wright's responsibility is narrower: configure and invoke the LPP provider,
preserve provider provenance in its result envelope, and expose explicit
capability errors when the provider cannot perform a requested operation. The
canonical Workshop catalog and WIR remain owned by `workshop-rs`.

Changes to OPY support belong in `opy-rs` first. A Wright change may add only
the corresponding provider integration or consumer regression test.
