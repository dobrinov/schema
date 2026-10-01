#!/usr/bin/env python3
"""CHANGELOG.md helpers.

  changelog.py html                 render the versions as an HTML fragment (for the website)
  changelog.py release X.Y.Z [DATE] turn [Unreleased] into [X.Y.Z] - DATE and start a new Unreleased
  changelog.py notes X.Y.Z          print the body of one version (for the GitHub release)

Only the subset of Markdown the changelog uses is supported: `## [version] - date`
headings, `### Group` headings, `- ` bullets (wrapped lines are indented), paragraphs,
`code`, **bold** and [links](url).
"""
import datetime
import html
import pathlib
import re
import sys

PATH = pathlib.Path(__file__).resolve().parent.parent / "CHANGELOG.md"
VERSION_RE = re.compile(r"^## \[([^\]]+)\](?:\s*-\s*(\S+))?\s*$")


def sections(text):
    """[(version, date, body_lines)] in file order; the preamble is dropped."""
    out = []
    for line in text.splitlines():
        m = VERSION_RE.match(line)
        if m:
            out.append((m.group(1), m.group(2), []))
        elif out and not re.match(r"^\[[^\]]+\]:\s", line):
            out[-1][2].append(line)
    return out


def inline(s):
    s = html.escape(s, quote=False)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    s = re.sub(r"\*\*([^*]+)\*\*", r"<b>\1</b>", s)
    s = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", r'<a href="\2">\1</a>', s)
    return s


def blocks(lines):
    """Group lines into ('h3', text) / ('li', text) / ('p', text)."""
    out = []
    for line in lines:
        if not line.strip():
            out.append(None)
        elif line.startswith("### "):
            out.append(("h3", line[4:].strip()))
        elif line.startswith("- "):
            out.append(("li", line[2:].strip()))
        elif out and out[-1] and line.startswith("  "):
            out[-1] = (out[-1][0], out[-1][1] + " " + line.strip())
        elif out and out[-1] and out[-1][0] == "p":
            out[-1] = ("p", out[-1][1] + " " + line.strip())
        else:
            out.append(("p", line.strip()))
    return [b for b in out if b]


def render_html(text, shown=2):
    """Releases as <article>s; all but the newest `shown` fold into a <details>."""
    parts = []
    count = 0
    for version, date, body in sections(text):
        bs = blocks(body)
        if version.lower() == "unreleased" and not any(k == "li" for k, _ in bs):
            continue
        anchor = "unreleased" if version.lower() == "unreleased" else "v" + version
        title = "Unreleased" if version.lower() == "unreleased" else "v" + html.escape(version)
        when = f' <time datetime="{date}">{date}</time>' if date else ' <span class="tag">on main</span>'
        if count == shown:
            parts.append('<details class="older">\n<summary>Older releases</summary>')
        count += 1
        parts.append(f'<article class="release" id="{anchor}">\n  <h3>{title}{when}</h3>')
        in_list = False
        for kind, s in bs:
            if kind == "li":
                if not in_list:
                    parts.append("  <ul>")
                    in_list = True
                parts.append(f"    <li>{inline(s)}</li>")
                continue
            if in_list:
                parts.append("  </ul>")
                in_list = False
            if kind == "h3":
                parts.append(f'  <h4 class="group {s.lower()}">{inline(s)}</h4>')
            else:
                parts.append(f"  <p>{inline(s)}</p>")
        if in_list:
            parts.append("  </ul>")
        parts.append("</article>")
    if count > shown:
        parts.append("</details>")
    return "\n".join(parts)


def release(text, version, date):
    if not re.search(r"^## \[Unreleased\]", text, re.M):
        sys.exit("CHANGELOG.md has no ## [Unreleased] section")
    unreleased = next((b for v, _, b in sections(text) if v.lower() == "unreleased"), [])
    if not any(l.startswith("- ") for l in unreleased):
        sys.exit("CHANGELOG.md: nothing under [Unreleased] — add the changes for this release first")
    text = re.sub(r"^## \[Unreleased\].*$", f"## [Unreleased]\n\n## [{version}] - {date}", text, count=1, flags=re.M)
    # link references: Unreleased compares from the new tag; the new version gets its own
    m = re.search(r"^\[Unreleased\]:\s*(\S+)/compare/(\S+)\.\.\.HEAD\s*$", text, re.M)
    if m:
        base, prev = m.group(1), m.group(2)
        text = text.replace(
            m.group(0),
            f"[Unreleased]: {base}/compare/v{version}...HEAD\n[{version}]: {base}/compare/{prev}...v{version}",
        )
    return text


def notes(text, version):
    for v, _, body in sections(text):
        if v == version:
            return "\n".join(body).strip() + "\n"
    sys.exit(f"CHANGELOG.md has no [{version}] section")


def main():
    args = sys.argv[1:]
    text = PATH.read_text()
    if args[:1] == ["html"]:
        print(render_html(text))
    elif args[:1] == ["release"] and len(args) >= 2:
        date = args[2] if len(args) > 2 else datetime.date.today().isoformat()
        PATH.write_text(release(text, args[1], date))
    elif args[:1] == ["notes"] and len(args) == 2:
        sys.stdout.write(notes(text, args[1].lstrip("v")))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
