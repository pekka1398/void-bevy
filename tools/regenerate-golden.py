#!/usr/bin/env python3
"""Run an owning TS golden script against an explicit reference checkout.

Only this workspace's crates receive generated files. The old reference sources
are read through temporary symlinks; no scripts or data are written into it.
"""
import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    workspace = Path(__file__).resolve().parents[1]
    scripts = {p.stem: p for p in (workspace / "golden").glob("*.ts")}
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-root", required=True, type=Path,
                        help="old VOID checkout with lab/ and installed Node dependencies")
    parser.add_argument("name", choices=sorted(scripts), help="golden script name, without .ts")
    args = parser.parse_args()
    reference = args.reference_root.resolve(strict=True)
    tsx = reference / "node_modules/.bin/tsx"
    if not (reference / "lab").is_dir() or not tsx.is_file():
        parser.error("reference must contain lab/ and node_modules/.bin/tsx; install its Node dependencies first")
    with tempfile.TemporaryDirectory(prefix="void-golden-reference-") as directory:
        stage = Path(directory)
        for child in reference.iterdir():
            if child.name not in {".git", "lab"}:
                (stage / child.name).symlink_to(child, target_is_directory=child.is_dir())
        labs = stage / "lab"
        labs.mkdir()
        for child in (reference / "lab").iterdir():
            if child.name != "void-bevy":
                (labs / child.name).symlink_to(child, target_is_directory=child.is_dir())
        project = labs / "void-bevy"
        project.mkdir()
        shutil.copytree(workspace / "golden", project / "golden")
        (project / "crates").symlink_to(workspace / "crates", target_is_directory=True)
        print(f"Reference: {reference}\nOutput: {workspace / 'crates'}", flush=True)
        subprocess.run([str(tsx), str(project / "golden" / scripts[args.name].name)],
                       cwd=stage, check=True)


if __name__ == "__main__":
    main()
