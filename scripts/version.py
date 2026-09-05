#!/usr/bin/env python3
"""Read and change the one version number this project has.

`backend/Cargo.toml` is the source of truth, because it is the only version anybody ever sees: it is what
`/api/health` reports, what `--version` prints, and what the UI footer shows. The frontend has to carry the
same number because it is compiled into that same binary, and its lockfile has to agree with its manifest.
That is four files holding one value, which is three opportunities to drift.

    version.py show          print the authoritative version
    version.py check         verify all four agree, exit 1 if they do not
    version.py set 0.2.0     move all four at once

`check` reads files and nothing else, no cargo and no npm, so CI can run it in a couple of seconds. Only
`set` needs the package managers, to regenerate the lockfiles rather than hand editing them.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

CARGO_TOML = os.path.join(ROOT, "backend", "Cargo.toml")
CARGO_LOCK = os.path.join(ROOT, "backend", "Cargo.lock")
PACKAGE_JSON = os.path.join(ROOT, "frontend", "package.json")
PACKAGE_LOCK = os.path.join(ROOT, "frontend", "package-lock.json")

CRATE = "on-air-record"

SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")


def read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def cargo_toml_version(text: str) -> str | None:
    """The version from the `[package]` table.

    Scoped to that table on purpose. A bare search for `version` would find the first dependency instead,
    which is both wrong and the kind of wrong that looks right.
    """
    in_package = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_package = stripped == "[package]"
            continue
        if in_package:
            match = re.match(r'version\s*=\s*"([^"]+)"', stripped)
            if match:
                return match.group(1)
    return None


def cargo_lock_version(text: str) -> str | None:
    """The version recorded for this crate's own entry in the lockfile."""
    for block in text.split("[[package]]"):
        if re.search(rf'^\s*name\s*=\s*"{re.escape(CRATE)}"\s*$', block, re.MULTILINE):
            match = re.search(r'^\s*version\s*=\s*"([^"]+)"\s*$', block, re.MULTILINE)
            if match:
                return match.group(1)
    return None


def package_json_version(text: str) -> str | None:
    return json.loads(text).get("version")


def package_lock_versions(text: str) -> list[tuple[str, str | None]]:
    """Both places npm records the root package's version."""
    data = json.loads(text)
    return [
        ("frontend/package-lock.json (top level)", data.get("version")),
        ('frontend/package-lock.json (packages[""])', data.get("packages", {}).get("", {}).get("version")),
    ]


def collect() -> list[tuple[str, str | None]]:
    """Every declared version, authoritative one first."""
    found = [
        ("backend/Cargo.toml", cargo_toml_version(read(CARGO_TOML))),
        ("backend/Cargo.lock", cargo_lock_version(read(CARGO_LOCK))),
        ("frontend/package.json", package_json_version(read(PACKAGE_JSON))),
    ]
    found.extend(package_lock_versions(read(PACKAGE_LOCK)))
    return found


def authoritative() -> str:
    version = cargo_toml_version(read(CARGO_TOML))
    if version is None:
        raise SystemExit("could not find the version in the [package] table of backend/Cargo.toml")
    return version


def command_show() -> int:
    print(authoritative())
    return 0


def command_check() -> int:
    found = collect()
    expected = found[0][1]

    width = max(len(label) for label, _ in found)
    for label, value in found:
        agrees = value == expected
        print(f"  {label.ljust(width)}  {value or '(missing)'}{'' if agrees else '   <- disagrees'}")

    # Without this the table lands after the diagnosis, because the two streams are buffered separately.
    sys.stdout.flush()

    missing = [label for label, value in found if value is None]
    if missing:
        print(f"\nno version found in: {', '.join(missing)}", file=sys.stderr)
        return 1

    wrong = [label for label, value in found[1:] if value != expected]
    if wrong:
        print(
            f"\nbackend/Cargo.toml says {expected}, but {', '.join(wrong)} disagrees.\n"
            f"Cargo.toml is the source of truth. Run `python3 scripts/version.py set {expected}` to bring\n"
            "everything into line, then commit the result.",
            file=sys.stderr,
        )
        return 1

    print(f"\nall four agree on {expected}")
    return 0


def run(command: list[str], cwd: str) -> None:
    print(f"  $ {' '.join(command)}")
    result = subprocess.run(command, cwd=cwd)
    if result.returncode != 0:
        raise SystemExit(f"`{' '.join(command)}` failed with {result.returncode}")


def command_set(version: str) -> int:
    if not SEMVER.match(version):
        print(f"'{version}' is not a semver version, expected something like 0.2.0", file=sys.stderr)
        return 2

    text = read(CARGO_TOML)
    current = cargo_toml_version(text)
    if current is None:
        print("could not find the version in the [package] table of backend/Cargo.toml", file=sys.stderr)
        return 1

    # Replace only inside the [package] table, for the same reason the reader is scoped to it.
    package_table = re.compile(r"(\[package\][^\[]*?version\s*=\s*)\"[^\"]+\"", re.DOTALL)
    updated, count = package_table.subn(rf'\1"{version}"', text, count=1)
    if count != 1:
        print("could not rewrite the version in backend/Cargo.toml", file=sys.stderr)
        return 1
    with open(CARGO_TOML, "w", encoding="utf-8") as handle:
        handle.write(updated)
    print(f"backend/Cargo.toml   {current} -> {version}")

    data = json.loads(read(PACKAGE_JSON))
    previous = data.get("version")
    data["version"] = version
    with open(PACKAGE_JSON, "w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2)
        handle.write("\n")
    print(f"frontend/package.json  {previous} -> {version}")

    # The lockfiles are regenerated by the tools that own them. Hand editing them works right up until it
    # silently does not.
    print("refreshing lockfiles")
    run(["cargo", "update", "--package", CRATE, "--offline"], cwd=os.path.join(ROOT, "backend"))
    run(["npm", "install", "--package-lock-only", "--silent"], cwd=os.path.join(ROOT, "frontend"))

    print()
    return command_check()


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2

    action = argv[1]
    if action == "show" and len(argv) == 2:
        return command_show()
    if action == "check" and len(argv) == 2:
        return command_check()
    if action == "set" and len(argv) == 3:
        return command_set(argv[2])

    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
