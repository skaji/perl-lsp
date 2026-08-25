# C/C++ support: ALPHA

**Status: alpha.** Opt-in behind `--features cpp`, not built by default, and not
recommended for large projects yet. This answers the "good enough to ship?" gate
that `cpp-golive-map.md` ARC 5 leaves open — with a measurement, and the answer
today is *not on projects of Godot's size*.

## What works

Goto-def, cross-TU references, member and in-scope completion, hover, include
navigation, macro-aware extraction (config-variant macro model, splice-mapped
reparse), diagnostics, and the same SQLite warm-start path Perl uses. 1,676
tests pass with `--features cpp` (+64 over Perl-only); the pack e2e lane is
8 suites / 24 tests.

## Why alpha: measured

| project | C++ files | result |
|---|---:|---|
| [fmt](https://github.com/fmtlib/fmt) | 80 indexed | 4.2 s, 0.50 GB — fine |
| [Godot](https://github.com/godotengine/godot) | 7,041 | **did not complete in 4 minutes**; killed |

Godot's memory behaviour is **good** — RSS plateaus flat at ~2 GB across 7,041
files, which is better per-file than the Perl side manages. The problem is wall
time, and it is concentrated in individual files:

```
[stall] 66s on one unit: thirdparty/vulkan/include/vulkan/vulkan_handles.hpp
[stall] 34s on one unit: thirdparty/vulkan/include/vulkan/vulkan_raii.hpp
[stall] 32s on one unit: thirdparty/directx_headers/include/directx/d3d12.h
[stall] 31s on one unit: thirdparty/ufbx/ufbx.c
```

**Every stall is a large generated or vendored header.** `d3d12.h` alone is
1.5 MB. A per-file cost of 30–66 seconds is unusable interactively no matter
what the aggregate looks like.

Note the failure mode is the **opposite** of the Perl side's: C++ is
memory-healthy and wall-pathological; Perl's FHEM shape is memory-pathological
(`scaling-limits.md`). They share no mechanism, and the Perl scaling work
neither caused nor fixed the C++ one.

## What alpha means in practice

- Fine for small-to-medium projects. fmt-sized is comfortable.
- Expect multi-second-to-multi-minute stalls on projects vendoring large
  generated headers — Vulkan, DirectX, and similar SDK headers are the
  reliable trigger.
- Excluding `thirdparty/` from the workspace avoids most of it today, since
  that is where the giant headers live in every project we measured.
- No API or behaviour stability promise. `--features cpp` stays opt-in.

## What would move it past alpha

The per-file stall, specifically — an aggregate number will not settle it. The
gate is a large generated header (`d3d12.h`, `vulkan_handles.hpp`) analysed in
interactive time, and no measurement of total wall across a corpus substitutes
for that. Nobody has profiled where those 30–66 seconds go; that is the next
step and it has not been taken.
