#!/usr/bin/env python3
"""Turn the guides in docs/ into GitHub wiki pages.

A GitHub wiki is a **separate git repository**, `<repo>.wiki.git`. Nothing in `docs/` reaches it by being
merged to the default branch, which is why the guides render in the repository and the wiki stays empty.
This script produces the wiki repository's contents so a workflow can commit them.

Three things have to change on the way across:

- **Page names.** A wiki page is named by its filename, so `USER_GUIDE.md` becomes `User-Guide.md` and is
  addressed as `/wiki/User-Guide`.
- **Links between documents.** `[the user guide](USER_GUIDE.md)` is a valid relative link inside `docs/`
  and a dead one in the wiki. Links to pages that cross over become wiki links; links to anything that
  stays behind, such as DEVELOPMENT.md or the systemd unit, become absolute URLs into the repository.
- **The leading heading.** The wiki prints the page name above the body, so a `# Title` in the source
  would appear twice.

Images already use absolute raw URLs and need no rewriting, which is why they were written that way.

Usage: build_wiki.py <output directory>
"""

from __future__ import annotations

import os
import posixpath
import re
import sys

REPOSITORY = "shibbirweb/on-air-record"
BRANCH = "master"
BLOB = f"https://github.com/{REPOSITORY}/blob/{BRANCH}"

# Source path in the repository, mapped to the wiki page name it becomes.
PAGES: dict[str, str] = {
    "docs/SETUP.md": "Installation-and-Setup",
    "docs/USER_GUIDE.md": "User-Guide",
}

# How each page is introduced on the Home page and in the sidebar, in the order shown.
DESCRIPTIONS: list[tuple[str, str]] = [
    (
        "Installation-and-Setup",
        "Downloading and running it on macOS, Linux and Windows, choosing a port, and keeping it "
        "running as a service.",
    ),
    (
        "User-Guide",
        "How to use every part of the interface, with screenshots. Written for people who just want "
        "to listen.",
    ),
]

GENERATED_NOTE = (
    "<!-- Generated from docs/ in {repository}. Do not edit this page in the wiki: the next push to "
    "{branch} will overwrite it. Edit the source file instead. -->"
)

# A markdown link or image target, captured as (target, anchor).
LINK = re.compile(r"\]\(([^)\s#]+)(#[^)\s]*)?\)")

LEADING_COMMENT = re.compile(r"\A\s*<!--.*?-->\s*", re.DOTALL)
LEADING_H1 = re.compile(r"\A#\s+.*?\n+", re.DOTALL)


def rewrite_link(source_path: str, target: str, anchor: str) -> str:
    """Point one link at wherever its destination ended up."""
    if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.IGNORECASE) or target.startswith("//"):
        return f"]({target}{anchor})"

    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(source_path), target))

    if resolved in PAGES:
        return f"]({PAGES[resolved]}{anchor})"

    # Everything else stays in the repository, so it needs an absolute URL to survive the move.
    return f"]({BLOB}/{resolved}{anchor})"


def convert(source_path: str, text: str) -> str:
    text = LEADING_COMMENT.sub("", text)
    text = LEADING_H1.sub("", text)
    text = LINK.sub(lambda m: rewrite_link(source_path, m.group(1), m.group(2) or ""), text)

    note = GENERATED_NOTE.format(repository=REPOSITORY, branch=BRANCH)
    return f"{note}\n\n{text.strip()}\n"


def home() -> str:
    lines = [
        GENERATED_NOTE.format(repository=REPOSITORY, branch=BRANCH),
        "",
        "# On Air Record",
        "",
        "A cross platform audio broadcast and DVR service. It records a microphone on one machine",
        "continuously, streams it live to any browser on the network, and lets you scrub back to any",
        "moment in the retention window on a CCTV style timeline.",
        "",
        "## Guides",
        "",
    ]
    for page, description in DESCRIPTIONS:
        lines.append(f"- **[{page.replace('-', ' ')}]({page})** {description}")
    lines += [
        "",
        "## Elsewhere",
        "",
        f"- [Source code](https://github.com/{REPOSITORY})",
        f"- [Releases](https://github.com/{REPOSITORY}/releases), with binaries for macOS, Linux and Windows",
        f"- [Report a problem](https://github.com/{REPOSITORY}/issues)",
        f"- [Developer documentation]({BLOB}/docs/DEVELOPMENT.md), for working on the code itself",
        "",
    ]
    return "\n".join(lines)


def sidebar() -> str:
    lines = [
        GENERATED_NOTE.format(repository=REPOSITORY, branch=BRANCH),
        "",
        "### On Air Record",
        "",
        "- [Home](Home)",
    ]
    lines += [f"- [{page.replace('-', ' ')}]({page})" for page, _ in DESCRIPTIONS]
    lines += [
        "",
        f"[Source]({BLOB}) &middot; [Issues](https://github.com/{REPOSITORY}/issues)",
        "",
    ]
    return "\n".join(lines)


def footer() -> str:
    return (
        f"These pages are generated from `docs/` in [the repository](https://github.com/{REPOSITORY}). "
        "Corrections are welcome as a pull request against the source file, since an edit made here would "
        "be overwritten by the next release.\n"
    )


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2

    out_dir = sys.argv[1]
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.makedirs(out_dir, exist_ok=True)

    written = []
    for source_path, page in sorted(PAGES.items()):
        full = os.path.join(root, source_path)
        if not os.path.exists(full):
            print(f"missing source: {source_path}", file=sys.stderr)
            return 1
        with open(full, encoding="utf-8") as handle:
            body = convert(source_path, handle.read())
        target = os.path.join(out_dir, f"{page}.md")
        with open(target, "w", encoding="utf-8") as handle:
            handle.write(body)
        written.append(f"{source_path} -> {page}.md")

    for name, content in (("Home.md", home()), ("_Sidebar.md", sidebar()), ("_Footer.md", footer())):
        with open(os.path.join(out_dir, name), "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(f"generated {name}")

    for line in written:
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
