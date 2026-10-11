"""Reject GUI, runtime and asynchronous runtime dependencies in rwk-core's normal graph."""
import argparse
import subprocess


def violations(tree):
    """Return sorted forbidden crate names from cargo tree --prefix none output."""
    names = {line.split()[0] for line in tree.splitlines() if line.strip()}
    return sorted(name for name in names if name.startswith(("gpui", "raios-surface-"))
                  or name in {"raios-core", "raios-runtime", "tauri", "wgpu", "tokio"})


def main():
    """Inspect the locked normal dependency graph and fail closed on cargo errors."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest")
    args = parser.parse_args()
    tree = subprocess.check_output(["cargo", "tree", "--manifest-path", args.manifest,
                                    "--locked", "-p", "rwk-core", "-e", "normal",
                                    "--prefix", "none"], text=True)
    found = violations(tree)
    if found:
        raise SystemExit("Forbidden rwk-core dependencies: " + ", ".join(found))
    print("rwk-core normal dependency boundary: ok")


if __name__ == "__main__":
    main()
