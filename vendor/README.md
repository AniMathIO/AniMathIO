`xattr/` is xattr 0.2.3 from crates.io, under its original MIT/Apache-2.0 licenses.
The sole source change is in `src/sys/mod.rs`: on Linux/Android, use
`libc::ENODATA` instead of the removed `libc::ENOATTR` alias. Preserve the macro's
platform selection on both branches. This keeps GPUI 0.2.2 buildable against the
current libc without changing the machine or pinning the entire graph to an old
libc release. Registry bookkeeping and CI files were omitted.
