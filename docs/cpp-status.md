# C/C++ support: BETA

**Status: beta**, as `--languages` reports and `language_driver.rs` declares.
Opt-in behind `--features cpp`; default builds stay Perl-only.

The tier is a promise about whether to trust the ANSWERS, and by the bar the
enum sets — *"broad gold coverage, known gaps documented"* — C/C++ meets it:
503 gold rows assert the verb surface with `lang-skip 0`, plus 1,676 unit
tests (+64 over Perl-only) and 24 e2e-cpp tests across 8 suites.

**What beta does not promise is scaling.** That is the honest answer to the
"good enough to ship?" gate `cpp-golive-map.md` ARC 5 leaves open: yes for
small-to-medium projects, not yet at Godot's size. A performance limit, not a
correctness one — which is why the tier stays beta rather than dropping.

## What works

Goto-def, cross-TU references, member and in-scope completion, hover, include
navigation, macro-aware extraction (config-variant macro model, splice-mapped
reparse), diagnostics, and the same SQLite warm-start path Perl uses. 1,676
tests pass with `--features cpp` (+64 over Perl-only); the pack e2e lane is
8 suites / 24 tests.

## The scaling limit: measured

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

## What this means in practice

- Fine for small-to-medium projects. fmt-sized is comfortable.
- Expect multi-second-to-multi-minute stalls on projects vendoring large
  generated headers — Vulkan, DirectX, and similar SDK headers are the
  reliable trigger.
- Excluding `thirdparty/` from the workspace avoids most of it today, since
  that is where the giant headers live in every project we measured.
- `--features cpp` stays opt-in; default builds are Perl-only.

## What would lift the scaling caveat

The per-file stall, specifically — an aggregate number will not settle it. The
gate is a large generated header (`d3d12.h`, `vulkan_handles.hpp`) analysed in
interactive time, and no measurement of total wall across a corpus substitutes
for that. Nobody has profiled where those 30–66 seconds go; that is the next
step and it has not been taken.
