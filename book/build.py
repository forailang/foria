#!/usr/bin/env python3
"""
Build the forai language book into a static HTML website.

Usage:
    python3 build.py [--out <dir>]

Requires: markdown  (auto-installed if missing)

Output: _site/ directory (or --out path)
Open:   file://<abs-path>/_site/index.html
"""

import os
import sys
import shutil
import argparse

# ── dependency bootstrap ────────────────────────────────────────────────────
try:
    import markdown as _md_mod
except ImportError:
    import subprocess

    def _try_pip(*extra_flags):
        result = subprocess.run(
            [sys.executable, "-m", "pip", "install", *extra_flags, "markdown"],
            capture_output=True,
        )
        return result.returncode == 0

    print("Installing 'markdown' package …")
    if not _try_pip() and not _try_pip("--user") and not _try_pip("--break-system-packages"):
        sys.exit(
            "ERROR: Could not install 'markdown'. "
            "Run manually:\n  pip install markdown\n"
            "or inside a venv:\n  python3 -m venv .venv && "
            "source .venv/bin/activate && pip install markdown && python3 build.py"
        )
    import markdown as _md_mod


BOOK_DIR = os.path.dirname(os.path.abspath(__file__))


# ── book structure scanner ──────────────────────────────────────────────────

def scan_book(book_dir):
    """
    Returns:
        list of (chapter_num, chapter_title, chapter_slug, pages)
        pages = list of (page_num, page_title, src_path, rel_html_path)
        rel_html_path is relative to _site/ root, e.g. "00-introduction/01-what-is-forai.html"
    """
    chapters = []
    for entry in sorted(os.scandir(book_dir), key=lambda e: e.name):
        if not entry.is_dir():
            continue
        if entry.name.startswith(('_', '.')):
            continue
        parts = entry.name.split('-', 1)
        if len(parts) != 2 or not parts[0].isdigit():
            continue
        chapter_num  = int(parts[0])
        chapter_slug = entry.name
        chapter_title = parts[1].replace('-', ' ').title()

        pages = []
        for page in sorted(os.scandir(entry.path), key=lambda e: e.name):
            if not page.name.endswith('.md'):
                continue
            stem = page.name[:-3]          # drop .md
            pparts = stem.split('-', 1)
            if len(pparts) != 2 or not pparts[0].isdigit():
                continue
            page_num   = int(pparts[0])
            page_title = pparts[1].replace('-', ' ').title()
            rel_html   = f"{chapter_slug}/{stem}.html"
            pages.append((page_num, page_title, page.path, rel_html))

        chapters.append((chapter_num, chapter_title, chapter_slug, pages))

    return chapters


# ── markdown → HTML ─────────────────────────────────────────────────────────

_EXTENSIONS = ['tables', 'fenced_code', 'toc', 'attr_list', 'def_list']

def md_to_html(src_path):
    with open(src_path, encoding='utf-8') as f:
        text = f.read()
    md = _md_mod.Markdown(extensions=_EXTENSIONS)
    return md.convert(text)


# ── navigation builder ───────────────────────────────────────────────────────

def build_nav(chapters, active_rel_html=None, base='..'):
    """
    active_rel_html: the rel_html_path of the current page, or None (index)
    base: '..' for chapter pages, '.' for index page
    """
    lines = ['<nav id="sidebar">']
    lines.append(f'<div class="sidebar-header">'
                 f'<a class="site-title" href="{base}/index.html">forai</a>'
                 f'<span class="subtitle">Language Reference</span>'
                 f'</div>')
    lines.append('<ul class="chapter-list">')

    for chapter_num, chapter_title, chapter_slug, pages in chapters:
        lines.append('<li class="chapter">')
        lines.append(f'<span class="chapter-label">'
                     f'{chapter_num:02d} — {chapter_title}</span>')
        lines.append('<ul class="page-list">')
        for page_num, page_title, _, rel_html in pages:
            active = 'class="active" ' if rel_html == active_rel_html else ''
            href = f'{base}/{rel_html}'
            lines.append(f'<li><a {active}href="{href}">{page_title}</a></li>')
        lines.append('</ul></li>')

    lines.append('</ul></nav>')
    return '\n'.join(lines)


# ── CSS ──────────────────────────────────────────────────────────────────────

CSS = """
:root {
  --sb-bg:      #16213e;
  --sb-text:    #a8b8d8;
  --sb-active:  #e94560;
  --sb-hover:   #60a5fa;
  --sb-w:       272px;
  --code-bg:    #1e1e2e;
  --accent:     #3b82f6;
  --border:     #e5e7eb;
  --font:       -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --mono:       "JetBrains Mono", "Fira Code", "Cascadia Code", ui-monospace, monospace;
}

* { box-sizing: border-box; margin: 0; padding: 0; }

body {
  font-family: var(--font);
  background: #f8f9fc;
  color: #111827;
  line-height: 1.75;
  display: flex;
}

/* ── sidebar ── */
#sidebar {
  width: var(--sb-w);
  min-height: 100vh;
  background: var(--sb-bg);
  color: var(--sb-text);
  position: fixed;
  top: 0; left: 0; bottom: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  scrollbar-width: thin;
  scrollbar-color: rgba(255,255,255,.1) transparent;
}

.sidebar-header {
  padding: 22px 20px 16px;
  border-bottom: 1px solid rgba(255,255,255,.06);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.site-title {
  font-size: 1.45rem;
  font-weight: 800;
  color: #fff;
  text-decoration: none;
  letter-spacing: -.03em;
}
.site-title:hover { color: var(--sb-active); }

.subtitle {
  font-size: 0.66rem;
  color: rgba(168,184,216,.45);
  text-transform: uppercase;
  letter-spacing: .1em;
}

.chapter-list { list-style: none; padding: 6px 0 24px; }

.chapter-label {
  display: block;
  padding: 10px 20px 3px;
  font-size: 0.65rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: .12em;
  color: rgba(168,184,216,.38);
  user-select: none;
}

.page-list { list-style: none; padding-bottom: 4px; }

.page-list a {
  display: block;
  padding: 4px 18px 4px 28px;
  font-size: 0.84rem;
  color: var(--sb-text);
  text-decoration: none;
  border-left: 2px solid transparent;
  transition: color .12s, background .12s;
}
.page-list a:hover {
  color: var(--sb-hover);
  background: rgba(96,165,250,.08);
}
.page-list a.active {
  color: var(--sb-active);
  border-left-color: var(--sb-active);
  background: rgba(233,69,96,.09);
  font-weight: 500;
}

/* ── content ── */
#content {
  margin-left: var(--sb-w);
  flex: 1;
  min-height: 100vh;
}

.content-inner {
  max-width: 880px;
  margin: 0 auto;
  padding: 52px 56px 100px;
}

/* typography */
h1 { font-size: 2.1rem; font-weight: 800; letter-spacing: -.03em; margin-bottom: 1.1rem; color: #0f172a; }
h2 { font-size: 1.35rem; font-weight: 700; margin: 2.2rem 0 .7rem; padding-bottom: 6px;
     border-bottom: 1px solid var(--border); color: #0f172a; }
h3 { font-size: 1.05rem; font-weight: 600; margin: 1.6rem 0 .45rem; color: #1e293b; }
h4 { font-size: .92rem; font-weight: 600; margin: 1.3rem 0 .35rem; color: #1e293b; }

p  { margin-bottom: .9rem; }
a  { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }

strong { font-weight: 600; color: #0f172a; }

ul, ol { margin: .4rem 0 .9rem 1.6rem; }
li { margin-bottom: .25rem; }

hr { border: none; border-top: 1px solid var(--border); margin: 2rem 0; }

blockquote {
  margin: 1rem 0;
  padding: 12px 18px;
  border-left: 4px solid var(--sb-active);
  background: #fff1f2;
  border-radius: 0 6px 6px 0;
  color: #7f1d1d;
  font-size: .92rem;
}

/* code */
code {
  font-family: var(--mono);
  font-size: .84em;
  background: #eef2ff;
  color: #4338ca;
  padding: 1px 5px;
  border-radius: 4px;
}

pre {
  background: var(--code-bg);
  border-radius: 10px;
  padding: 20px 24px;
  overflow-x: auto;
  margin: .8rem 0 1.4rem;
  font-size: .875rem;
  line-height: 1.65;
  box-shadow: 0 3px 14px rgba(0,0,0,.2);
}
pre code {
  background: none;
  color: #cdd6f4;
  padding: 0;
  font-size: inherit;
  border-radius: 0;
}

/* tables */
table {
  width: 100%;
  border-collapse: collapse;
  margin: .8rem 0 1.4rem;
  font-size: .875rem;
  border-radius: 8px;
  overflow: hidden;
  box-shadow: 0 1px 4px rgba(0,0,0,.06);
}
th {
  background: #1e293b;
  color: #f1f5f9;
  padding: 10px 14px;
  text-align: left;
  font-weight: 600;
  font-size: .78rem;
  letter-spacing: .04em;
}
td {
  padding: 9px 14px;
  border-bottom: 1px solid #f1f5f9;
  vertical-align: top;
}
tr:last-child td { border-bottom: none; }
tr:nth-child(even) td { background: #f8fafc; }
tr:hover td { background: #eff6ff; }

/* page footer nav */
.page-nav {
  margin-top: 3.5rem;
  padding-top: 1.5rem;
  border-top: 1px solid var(--border);
  display: flex;
  justify-content: space-between;
  font-size: .875rem;
}
.page-nav a {
  color: var(--accent);
  font-weight: 500;
}

/* index page */
.toc-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
  gap: 1.5rem;
  margin-top: 2rem;
}
.toc-card {
  background: #fff;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 20px 22px;
  box-shadow: 0 1px 3px rgba(0,0,0,.05);
}
.toc-card h2 {
  font-size: .8rem;
  text-transform: uppercase;
  letter-spacing: .1em;
  color: rgba(30,41,59,.45);
  margin: 0 0 10px;
  padding: 0;
  border: none;
  font-weight: 700;
}
.toc-card ul { list-style: none; margin: 0; }
.toc-card li { padding: 3px 0; font-size: .875rem; }
.toc-card a { color: var(--accent); }
.toc-card a:hover { text-decoration: underline; }

/* responsive */
@media (max-width: 800px) {
  #sidebar { display: none; }
  #content { margin-left: 0; }
  .content-inner { padding: 28px 20px 60px; }
  .toc-grid { grid-template-columns: 1fr; }
}
"""


# ── HTML templates ───────────────────────────────────────────────────────────

PAGE_TEMPLATE = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — forai Language Reference</title>
<style>{css}</style>
<link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/catppuccin-mocha.min.css">
<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
<script>document.addEventListener('DOMContentLoaded',()=>{{
  document.querySelectorAll('pre code').forEach(b=>{{
    // treat unknown 'fa' language as plaintext with some structure
    if(b.classList.contains('language-fa')) b.classList.add('language-plaintext');
    hljs.highlightElement(b);
  }});
}});</script>
</head>
<body>
{nav}
<div id="content"><div class="content-inner">
{body}
<div class="page-nav">{prev}{next}</div>
</div></div>
</body>
</html>
"""

INDEX_TEMPLATE = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>forai Language Reference</title>
<style>{css}</style>
</head>
<body>
{nav}
<div id="content"><div class="content-inner">
<h1>forai Language Reference</h1>
<p>A comprehensive reference for the forai dataflow language — covering syntax, semantics,
the standard library, and real-world patterns.</p>
<hr>
<div class="toc-grid">
{cards}
</div>
</div></div>
</body>
</html>
"""


def build_index_cards(chapters):
    parts = []
    for chapter_num, chapter_title, chapter_slug, pages in chapters:
        links = '\n'.join(
            f'<li><a href="{rel_html}">{page_title}</a></li>'
            for _, page_title, _, rel_html in pages
        )
        parts.append(
            f'<div class="toc-card">'
            f'<h2>{chapter_num:02d} — {chapter_title}</h2>'
            f'<ul>{links}</ul>'
            f'</div>'
        )
    return '\n'.join(parts)


# ── main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description='Build forai book → static HTML site.')
    parser.add_argument('--out', default=os.path.join(BOOK_DIR, '_site'),
                        help='Output directory (default: book/_site/)')
    args = parser.parse_args()
    out_dir = args.out

    # wipe and recreate output dir
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)
    os.makedirs(out_dir)

    chapters = scan_book(BOOK_DIR)
    if not chapters:
        print("ERROR: No chapters found. Run from the book/ directory.")
        sys.exit(1)

    # flatten pages for sequential prev/next
    all_pages = [(chslug, page) for _, _, chslug, pages in chapters for page in pages]
    total = len(all_pages)

    print(f"Building {len(chapters)} chapters, {total} pages → {out_dir}")

    # create chapter output dirs
    for _, _, chslug, _ in chapters:
        os.makedirs(os.path.join(out_dir, chslug), exist_ok=True)

    # render each page
    for i, (chslug, (pnum, ptitle, src_path, rel_html)) in enumerate(all_pages):
        nav  = build_nav(chapters, active_rel_html=rel_html, base='..')
        body = md_to_html(src_path)

        prev_html = next_html = ''
        if i > 0:
            _, (_, pt, _, ph) = all_pages[i - 1]
            prev_html = f'<a href="../{ph}">← {pt}</a>'
        if i < total - 1:
            _, (_, nt, _, nh) = all_pages[i + 1]
            next_html = f'<a href="../{nh}">{nt} →</a>'

        html = PAGE_TEMPLATE.format(
            title=ptitle, css=CSS, nav=nav, body=body,
            prev=prev_html, next=next_html,
        )
        with open(os.path.join(out_dir, rel_html), 'w', encoding='utf-8') as f:
            f.write(html)

        print(f"  [{i+1:03d}/{total}] {rel_html}")

    # render index
    nav   = build_nav(chapters, active_rel_html=None, base='.')
    cards = build_index_cards(chapters)
    with open(os.path.join(out_dir, 'index.html'), 'w', encoding='utf-8') as f:
        f.write(INDEX_TEMPLATE.format(css=CSS, nav=nav, cards=cards))

    print(f"\nDone!  open file://{os.path.join(out_dir, 'index.html')}")


if __name__ == '__main__':
    main()
