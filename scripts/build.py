#!/usr/bin/env python3
import os
import subprocess
import sys
import shlex

scripts_dir = os.path.dirname(__file__)
project_dir = os.path.abspath(os.path.join(scripts_dir, '..'))

build_release = len(sys.argv) > 1 and sys.argv[1] == "release"
wasm_strip = build_release
wasm_opt = build_release
package_name = "timberborn-platform-cruncher-gui"
wasm_out_name = "main"
wasm_opt_flags = ["-O2", "--fast-math"]

bin_name = package_name.replace("-", "_")
wasm_build_bin_path = os.path.join(
    project_dir,
    "target",
    "wasm32-unknown-unknown",
    "release" if build_release else "debug",
    f"{bin_name}.wasm"
)
wasm_out_dir = os.path.join(project_dir, "web")
wasm_out_bin_path = os.path.join(wasm_out_dir, f"{wasm_out_name}_bg.wasm")
js_out_bin_path = os.path.join(wasm_out_dir, f"{wasm_out_name}.js")


def once_if[T](cond: bool, item: T) -> tuple[T] | tuple:
    if cond:
        return (item,)
    else:
        return ()


# Note: wasm-bindgen-cli and the Rust crate versions must match exactly

print("Ensuring that rust-src is installed")
subprocess.run(shlex.split("rustup component add rust-src --toolchain nightly")).check_returncode()

os.environ.update(
    {"RUSTFLAGS": "-C target-feature=+atomics"}
)

print("Building")
subprocess.run(
    [
        "cargo",
        "+nightly",
        "build",

        "--package",
        "timberborn-platform-cruncher-gui",

        "--lib",

        "--target",
        "wasm32-unknown-unknown",

        *once_if(build_release, "--release"),

        "--no-default-features",

        "--features",
        "web_app",

        "-Zbuild-std=std,panic_abort",

        # Temporary patch for rustsat depending on cpu-time, which doesn't work on WASM
        # Note: currently reports that the patch was not applied when -Zbuild-std is active
        "--config",
        'patch.crates-io.cpu-time.path = "patches/cpu-time-patch"'
    ],
    cwd=project_dir,
).check_returncode()

# Note the usage of `--target no-modules` here which is required for passing
# the memory import to each Wasm module.
print("Running wasm-bindgen")
subprocess.run(
    [
        "wasm-bindgen",

        wasm_build_bin_path,

        "--out-dir",
        wasm_out_dir,

        "--out-name",
        wasm_out_name,

        "--target",
        "no-modules",

        "--no-typescript",
    ],
    cwd=project_dir,
).check_returncode()

# TODO Currently breaks wasm_opt
# if wasm_strip:
#     print("Running wasm-strip")
#     subprocess.run(
#         [
#             "wasm-strip",
#             wasm_out_bin_path,
#         ]
#     ).check_returncode()

if wasm_opt:
    print("Running wasm-opt")
    subprocess.run(
        [
            "wasm-opt",

            wasm_out_bin_path,

            *wasm_opt_flags,

            "-o",
            wasm_out_bin_path,
        ],
    ).check_returncode()
