"""Builds Halo RT as one executable: dist/halo_rt.exe.

It carries the game (OpenCE's halo.exe and SDL3.dll), the Rust simulation
(the 32-bit halo_sim.dll) and the DLSS runtime (the `bundle` and `dlss`
features: build.rs embeds them, src/bundle.rs unpacks them at start-up).
Run, it starts the game hidden, with every simulation system in Rust, and
draws it path traced; at the game's menus, or on a level named
(`halo_rt.exe b30`). The game's data is never embedded: the maps come from
HALO_MAPS, --maps-dir, or a maps folder beside the executable.

    python tools/package.py [--opence opence] [--no-dlss]

Builds, in order: OpenCE (in opence/: configure.py the first time, then
ninja, with LLVM's clang on the PATH), halo_sim for i686-pc-windows-msvc, then the viewer.
"""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LLVM = Path(r"C:\Program Files\LLVM\bin")


def run(command, cwd, env=None):
    print(f"> {' '.join(str(part) for part in command)}  (in {cwd})", flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--opence", type=Path, default=REPO / "opence", help="the OpenCE checkout")
    parser.add_argument("--no-dlss", action="store_true", help="without DLSS")
    args = parser.parse_args()
    opence = args.opence.resolve()

    env = dict(os.environ)
    if LLVM.is_dir():
        env["PATH"] = f"{LLVM}{os.pathsep}{env.get('PATH', '')}"
    if not (opence / "build.ninja").is_file():
        run([sys.executable, "configure.py"], cwd=opence, env=env)
    run([sys.executable, "-m", "ninja"], cwd=opence, env=env)
    run(["cargo", "build", "-p", "halo_sim", "--release", "--target", "i686-pc-windows-msvc"], cwd=REPO)
    features = "bundle" if args.no_dlss else "dlss,bundle"
    env["HALO_RT_OPENCE_BUILD"] = str(opence / "build" / "windows")
    run(["cargo", "build", "--release", "--features", features], cwd=REPO, env=env)

    dist = REPO / "dist"
    dist.mkdir(exist_ok=True)
    target = dist / "halo_rt.exe"
    shutil.copy2(REPO / "target" / "release" / "halo_rt.exe", target)
    print(f"\n{target} ({target.stat().st_size / 1e6:.0f} MB)")


if __name__ == "__main__":
    main()
