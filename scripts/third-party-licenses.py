#!/usr/bin/env python3
"""Writes crates/keel/THIRD-PARTY-LICENSES.txt: the notices and licences of
the software built into Keel.app, which most of those licences ask to be
passed on with the app.

It follows the app's dependencies as they are compiled for a Mac with Apple
Silicon, leaving out what only runs while building (build scripts, macros).
Each package gives the licence files it carries; for the few that carry none,
and for what comes compiled inside a package (V8 and its libraries in `v8`),
the texts are in scripts/licenses/<package>/. Identical texts are written
once, with the packages they cover.

Run it after changing dependencies; a test in crates/keel fails until then.

    scripts/third-party-licenses.py          # writes the file
    scripts/third-party-licenses.py --check  # only says whether it is current
"""

import hashlib
import json
import re
import subprocess
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/keel/THIRD-PARTY-LICENSES.txt"
EXTRA = ROOT / "scripts/licenses"
TARGET = "aarch64-apple-darwin"
LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense)([-._].*)?$", re.IGNORECASE)
RULE = "-" * 76
# Files of others that travel with the app without being compiled into it:
# a name for the notices, and the licence they come with.
BUNDLED = [
    ("Material Icon Theme 5.40.0 (file icons)", ROOT / "crates/keel/icons/material/LICENSE.txt"),
]


def fingerprint(lock: str) -> str:
    """The third-party packages of Cargo.lock, hashed: it changes when a
    dependency is added, removed or updated, not when Keel's own version
    does. The test in crates/keel/src/licenses.rs computes the same."""
    found, package = [], {}
    for line in lock.splitlines() + ["[[package]]"]:
        if line.strip() == "[[package]]":
            if "source" in package:
                found.append(f"{package['name']} {package['version']}")
            package = {}
            continue
        match = re.match(r'^(name|version|source) = "(.*)"$', line)
        if match:
            package[match[1]] = match[2]
    return hashlib.sha256("\n".join(sorted(found)).encode()).hexdigest()


def tidy(text: str) -> str:
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    return "\n".join(lines).strip("\n")


def shipped(metadata: dict) -> list[dict]:
    """The packages compiled into the keel binary, ours left out."""
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    members = set(metadata["workspace_members"])
    vendor = ROOT / "vendor"

    def ours(package: dict) -> bool:
        return package["id"] in members and vendor not in Path(package["manifest_path"]).parents

    def macro(package: dict) -> bool:
        return any("proc-macro" in target["kind"] for target in package["targets"])

    start = next(id for id in members if packages[id]["name"] == "keel")
    seen, queue, out = {start}, [start], []
    while queue:
        node = nodes[queue.pop()]
        for dep in node["deps"]:
            if not any(kind["kind"] is None for kind in dep["dep_kinds"]):
                continue  # only for tests or for building
            package = packages[dep["pkg"]]
            if dep["pkg"] in seen or macro(package):
                continue
            seen.add(dep["pkg"])
            queue.append(dep["pkg"])
            if not ours(package):
                out.append(package)
    return sorted(out, key=lambda package: (package["name"].lower(), package["version"]))


# How a text came: read from the package, written in scripts/licenses (kept
# whole, with the name of what it covers), or made from the declared licence.
READ, KEPT, MADE = "read", "kept", "made"


def texts_of(package: dict) -> list[tuple[str, str]]:
    """The licence texts a package gives, each with how it came."""
    folder = Path(package["manifest_path"]).parent
    texts = []
    extra = EXTRA / package["name"]
    if extra.is_dir():
        texts += [(tidy(path.read_text(errors="replace")), KEPT) for path in sorted(extra.glob("*.txt"))]
    paths = [path for path in sorted(folder.iterdir()) if path.is_file() and LICENCE_FILE.match(path.name)]
    if package.get("license_file"):
        named = folder / package["license_file"]
        if named.is_file() and named not in paths:
            paths.append(named)
    # A few files only hold a path ("../LICENSE") or the licence's name.
    texts += [(text, READ) for text in (tidy(path.read_text(errors="replace")) for path in paths) if len(flat(text)) >= 100]
    return texts or fallback(package)


def fallback(package: dict) -> list[tuple[str, str]]:
    """For a package that carries no licence file: the standard text of the
    licence it declares (where it offers MIT, that one), with its authors as
    the package lists them."""
    offered = re.split(r"\s+OR\s+|/", package.get("license") or "")
    for licence in ("MIT",):
        if licence not in offered:
            continue
        holders = [re.sub(r"\s*<[^>]*>", "", author).strip() for author in package.get("authors") or []]
        holders = ", ".join(holder for holder in holders if holder) or f"the {package['name']} authors"
        template = (EXTRA / "spdx" / f"{licence}.txt").read_text()
        return [(tidy(template.replace("{copyright}", f"Copyright (c) {holders}")), MADE)]
    return []


COPYRIGHT = re.compile(r"^\s*(?:Copyright\b|COPYRIGHT\s*(?:\([cC]\)|©|\d)|\([cC]\)\s|©)")
PLACEHOLDER = re.compile(r"\[yyyy\]|\{yyyy\}|name of copyright owner", re.IGNORECASE)
MIT_TITLE = re.compile(r"^\s*(?:the\s+)?mit\s+licen[cs]e(?:\s*\(mit\))?\s*$", re.IGNORECASE)


def flat(text: str) -> str:
    return " ".join(text.split())


def standard(text: str) -> tuple[str, str, list[str]] | None:
    """For the MIT and Apache 2.0 licences, which only differ between
    packages in their copyright lines and spacing: a key shared by all the
    copies, the text without its copyright lines, and those lines."""
    whole = flat(text)
    apache = "Apache License" in whole and "Version 2.0, January 2004" in whole
    mit = "Permission is hereby granted, free of charge" in whole and "THE SOFTWARE IS PROVIDED" in whole
    if apache == mit:
        return None
    lines = text.split("\n")

    def owners_in(part: str) -> list[str]:
        return [line.strip() for line in part.split("\n") if COPYRIGHT.match(line) and not PLACEHOLDER.search(line)]

    if apache:
        if "END OF TERMS AND CONDITIONS" not in text:
            return None
        start, end = text.index("Apache License"), text.index("END OF TERMS AND CONDITIONS")
        # The terms themselves have a "(c)" clause: owners are only looked
        # for around them.
        owners = owners_in(text[:start]) + owners_in(text[end:])
        return "Apache-2.0", tidy(text[start:end] + "END OF TERMS AND CONDITIONS"), owners
    owners = owners_in(text)
    body = tidy("\n".join(line for line in lines if not COPYRIGHT.match(line) and not MIT_TITLE.match(line)))
    body = re.sub(r"\n{3,}", "\n\n", body)
    if len(flat(body)) > 1300:
        return None  # MIT with more in it, such as other notices: kept whole
    return "MIT " + flat(body), body, owners


def wrap(names: list[str]) -> str:
    return "\n".join(textwrap.wrap(", ".join(names), 76, break_on_hyphens=False))


def render(lock: str) -> str:
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    # key -> (body, packages, copyright lines)
    groups: dict[str, tuple[str, list[str], list[str]]] = {}
    missing = []
    packages = shipped(metadata)
    for package in packages:
        name = f"{package['name']} {package['version']}"
        texts = texts_of(package)
        if not texts:
            missing.append(f"{name} ({package.get('license')})")
        for text, how in texts:
            found = standard(text) if how != KEPT else None
            key, body, owners = found if found else ("text " + flat(text), text, [])
            body_, names, lines = groups.setdefault(key, (body, [], []))
            if name not in names:
                names.append(name)
            note = "; from its declared licence: the package has no licence file" if how == MADE else ""
            for owner in owners:
                line = f"{owner} ({name}{note})"
                if line not in lines:
                    lines.append(line)
    for name, licence in BUNDLED:
        text = tidy(licence.read_text())
        found = standard(text)
        key, body, owners = found if found else ("text " + flat(text), text, [])
        body_, names, lines = groups.setdefault(key, (body, [], []))
        names.append(name)
        for owner in owners:
            line = f"{owner} ({name})"
            if line not in lines:
                lines.append(line)
    if missing:
        sys.exit(
            "No licence text for:\n  "
            + "\n  ".join(missing)
            + f"\nPut it in {EXTRA.relative_to(ROOT)}/<package>/, as its authors publish it."
        )

    head = f"""Third-party software in Keel

Keel is built on open source software. These are the notices and licences
of everything compiled into the app, as their authors ask them to be passed
on: {len(packages)} packages, and the libraries that come compiled inside them,
and of the files of others that travel with it, such as its file icons.
Each text covers the packages listed above it; the MIT and Apache 2.0
licences, the same for many packages, are written once, after the copyright
lines of all the packages they cover.

Generated by scripts/third-party-licenses.py for {TARGET}.
Dependencies: {fingerprint(lock)}
"""
    sections = []
    for body, names, lines in sorted(groups.values(), key=lambda group: (len(group[1]) < 2, group[1][0].lower())):
        owners = "".join(f"{line}\n" for line in lines)
        sections.append(f"{RULE}\n{wrap(names)}\n{RULE}\n\n{owners}{chr(10) if owners else ''}{body}\n")
    return head + "\n" + "\n".join(sections)


def main() -> None:
    lock = (ROOT / "Cargo.lock").read_text()
    if "--check" in sys.argv:
        current = OUT.is_file() and f"Dependencies: {fingerprint(lock)}" in OUT.read_text()
        print("current" if current else "out of date: run scripts/third-party-licenses.py")
        sys.exit(0 if current else 1)
    text = render(lock)
    OUT.write_text(text)
    print(f"{OUT.relative_to(ROOT)}: {len(text) // 1024} kB")


if __name__ == "__main__":
    main()
