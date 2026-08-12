#!/usr/bin/env python3
"""Build the static /docs/ pages for os.farfield.systems from the
farfield-os repo's markdown. Markdown → HTML via `npx marked --gfm`,
wrapped in a shared dark brand layout with a sidebar. Intra-repo .md links
are rewritten to the clean /docs/ URLs; the README banner image is dropped
(the site has its own hero)."""

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(sys.argv[1])
OUT = Path(sys.argv[2])

PAGES = [
    # (slug, title, source, sidebar label)
    ("", "Overview", REPO / "README.md", "Overview"),
    ("helpers", "Helpers", REPO / "docs/helpers.md", "Helpers"),
    ("configuration", "Configuration", REPO / "docs/configuration.md", "Configuration"),
    ("troubleshooting", "Troubleshooting", REPO / "docs/troubleshooting.md", "Troubleshooting"),
]

LAYOUT = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="theme-color" content="#0e192a">
<title>{title} — Farfield OS</title>
<link rel="icon" href="data:image/svg+xml,<svg%20xmlns='http://www.w3.org/2000/svg'%20viewBox='0%200%2064%2064'><rect%20width='64'%20height='64'%20rx='12'%20fill='%230e222d'/><circle%20cx='32'%20cy='32'%20r='13'%20fill='none'%20stroke='%23f3e5d1'%20stroke-width='2'%20opacity='.8'/><circle%20cx='32'%20cy='32'%20r='3.5'%20fill='%23e59f67'/></svg>">
<meta name="description" content="{title} — Farfield OS documentation. An opinionated home-server bootstrap for Arch Linux.">
<link rel="canonical" href="https://os.farfield.systems/docs/{canon}">
<meta property="og:type" content="article">
<meta property="og:site_name" content="farfield">
<meta property="og:title" content="{title} — Farfield OS">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=Inter:wght@400;500;600&family=Newsreader:opsz,wght@6..72,400;6..72,500&display=swap">
<link rel="stylesheet" href="/docs/docs.css">
</head>
<body>
<header class="bar">
  <a href="/" class="brand">farfield&nbsp;os</a>
  <nav class="crumbs" aria-label="Site">
    <a href="/docs/"{active_docs}>Docs</a>
    <a href="https://github.com/iammatthias/farfield-os">Source</a>
    <a href="https://farfield.systems/">Fleet</a>
  </nav>
</header>
<div class="layout">
  <aside>
    <nav class="side" aria-label="Docs">
{nav}
    </nav>
  </aside>
  <main class="doc">
{content}
  </main>
</div>
<footer>
  <span class="note">Farfield OS · MIT</span>
</footer>
</body>
</html>
"""

CSS = """/* os.farfield.systems/docs — the dark brand, set for reading. */
*,*::before,*::after{margin:0;padding:0;box-sizing:border-box}
:root{
  --sky:#0e192a;
  --paper:#f3e5d1;
  --horizon:#e59f67;
  --mist:#9babb3;
  --border:rgba(243,229,209,.14);
  --wash:rgba(243,229,209,.05);
}
body{
  background:var(--sky);
  color:var(--paper);
  font-family:"Inter",system-ui,sans-serif;
  font-size:1rem;
  line-height:1.6;
}
a{color:var(--paper);text-decoration:none;border-bottom:1px solid rgba(243,229,209,.35)}
a:hover{border-color:rgba(243,229,209,.75)}
a:focus-visible{outline:2px solid var(--horizon);outline-offset:3px}

.bar{
  display:flex;justify-content:space-between;align-items:baseline;gap:1rem;flex-wrap:wrap;
  max-width:72rem;margin-inline:auto;
  padding:1.4rem clamp(1.25rem,4vw,2.5rem) 1.2rem;
  border-bottom:1px solid var(--border);
}
.brand{font-weight:500;letter-spacing:-.01em;border:0}
.crumbs{display:flex;gap:1.5rem}
.crumbs a{font-size:.9rem;font-weight:500;border:0;opacity:.8}
.crumbs a:hover,.crumbs a[aria-current]{opacity:1}

.layout{
  max-width:72rem;margin-inline:auto;
  display:grid;grid-template-columns:13rem minmax(0,1fr);gap:clamp(2.5rem,5vw,4.5rem);
  padding:3.5rem clamp(1.25rem,4vw,2.5rem) 6rem;
}
aside{position:sticky;top:1.5rem;align-self:start}
.side{display:flex;flex-direction:column;gap:.15rem}
.side a{
  border:0;padding:.35rem .6rem;border-radius:4px;
  font-size:.92rem;opacity:.75;
}
.side a:hover{opacity:1;background:var(--wash)}
.side a[aria-current]{opacity:1;background:var(--wash);font-weight:500}
@media (max-width:52rem){
  .layout{grid-template-columns:1fr;gap:1.5rem}
  aside{position:static}
  .side{flex-direction:row;flex-wrap:wrap;gap:.25rem}
}

.doc{max-width:42rem}
.doc h1{
  font-family:"Newsreader",Georgia,serif;font-weight:500;
  font-size:clamp(1.9rem,4vw,2.5rem);line-height:1.05;letter-spacing:-.02em;
  margin-bottom:1.2rem;
}
.doc h2{
  font-family:"Newsreader",Georgia,serif;font-weight:500;
  font-size:1.5rem;letter-spacing:-.015em;line-height:1.15;
  margin:4rem 0 1rem;padding-top:2rem;border-top:1px solid var(--border);
}
.doc h3{font-size:1.02rem;font-weight:600;margin:2.4rem 0 .6rem}
.doc p{margin:1rem 0;color:rgba(243,229,209,.85);line-height:1.65}
.doc strong{color:var(--paper);font-weight:600}
.doc ul,.doc ol{margin:1rem 0;padding-left:1.5rem;color:rgba(243,229,209,.85)}
.doc li{margin:.5rem 0;line-height:1.6}
.doc li::marker{color:var(--mist)}
.doc img{max-width:100%;border-radius:6px}

.doc code{
  font-family:"IBM Plex Mono",ui-monospace,monospace;font-size:.86em;
  background:var(--wash);border-radius:3px;padding:.12em .35em;
}
.doc pre{
  background:var(--wash);border:1px solid var(--border);border-radius:6px;
  padding:1.1rem 1.25rem;overflow-x:auto;margin:1.4rem 0;
  font-size:.85rem;line-height:1.7;
}
.doc pre code{background:none;padding:0;font-size:1em;color:var(--paper)}

.doc table{border-collapse:collapse;width:100%;margin:1rem 0;font-size:.92rem;display:block;overflow-x:auto}
.doc th{
  font-family:"IBM Plex Mono",ui-monospace,monospace;text-transform:uppercase;
  font-size:.7rem;letter-spacing:.08em;font-weight:500;color:var(--mist);
  text-align:left;padding:.5rem .8rem;border-bottom:1px solid rgba(243,229,209,.3);
}
.doc td{padding:.5rem .8rem;border-bottom:1px solid var(--border);color:rgba(243,229,209,.85)}
.doc blockquote{border-left:2px solid rgba(229,159,103,.55);padding-left:1rem;margin:1rem 0;opacity:.85}
.doc hr{border:0;border-top:1px solid var(--border);margin:2rem 0}

.observation{margin:2.2rem 0 0}
.observation svg{display:block;width:100%;height:auto}
.observation figcaption{
  font-family:"IBM Plex Mono",ui-monospace,monospace;
  font-size:.7rem;font-weight:500;letter-spacing:.1em;text-transform:uppercase;
  color:var(--mist);margin-top:.9rem;
}

footer{max-width:72rem;margin-inline:auto;padding:0 clamp(1.25rem,4vw,2.5rem) 2.5rem}
.note{
  font-family:"IBM Plex Mono",ui-monospace,monospace;
  font-size:.72rem;letter-spacing:.14em;text-transform:uppercase;color:var(--mist);
}
"""


def md_to_html(path: Path) -> str:
    return subprocess.run(
        ["npx", "--yes", "marked", "--gfm"],
        input=path.read_text(),
        capture_output=True, text=True, check=True,
    ).stdout


OVERVIEW_DIAGRAM = """<figure class="observation" aria-label="System diagram: the internet reaches the box through a Cloudflare tunnel; the tailnet reaches it through Tailscale; both land on Caddy, which routes to projects on host ports. A display gets the kiosk board.">
<svg viewBox="0 0 700 280" role="img">
  <g style="font-family:'IBM Plex Mono',monospace;font-size:11px" fill="#f3e5d1">
    <!-- two ways in -->
    <circle cx="48" cy="80" r="4.5" fill="#f3e5d1"/>
    <text x="48" y="58" text-anchor="middle" opacity="0.6">internet</text>
    <path d="M 56 82 C 120 100, 180 118, 240 132" fill="none" stroke="#f3e5d1" stroke-width="1" stroke-dasharray="4 4" opacity="0.5"/>
    <text x="132" y="92" opacity="0.6">cloudflare tunnel</text>
    <circle cx="48" cy="208" r="4.5" fill="none" stroke="#f3e5d1" opacity="0.8"/>
    <text x="48" y="238" text-anchor="middle" opacity="0.6">tailnet</text>
    <path d="M 56 206 C 120 190, 180 172, 240 156" fill="none" stroke="#f3e5d1" stroke-width="1" stroke-dasharray="2 4" opacity="0.5"/>
    <text x="132" y="206" opacity="0.6">tailscale</text>
    <!-- caddy: signal in field -->
    <circle cx="256" cy="144" r="11" fill="none" stroke="#e59f67" stroke-width="1.2" opacity="0.9"/>
    <circle cx="256" cy="144" r="3" fill="#e59f67"/>
    <text x="256" y="176" text-anchor="middle" opacity="0.7">caddy</text>
    <!-- host ports -->
    <g stroke="#f3e5d1" stroke-width="0.75" opacity="0.25">
      <line x1="268" y1="140" x2="420" y2="72"/>
      <line x1="269" y1="144" x2="420" y2="144"/>
      <line x1="268" y1="148" x2="420" y2="216"/>
    </g>
    <g opacity="0.85">
      <circle cx="424" cy="72" r="3.5"/><text x="434" y="76">the farfield fleet · :8787–:8801</text>
      <circle cx="424" cy="144" r="3.5"/><text x="434" y="148">your projects · add-site &lt;name&gt; &lt;port&gt;</text>
      <circle cx="424" cy="216" r="3.5"/><text x="434" y="220">previews · tailnet-only HTTPS</text>
    </g>
    <!-- the kiosk, off to the side -->
    <circle cx="256" cy="252" r="3.5" fill="none" stroke="#f3e5d1" opacity="0.7"/>
    <text x="266" y="256" opacity="0.6">kiosk board · if a display is attached</text>
  </g>
</svg>
<figcaption>Observation 01 · one box, two ways in, opt-in public surface</figcaption>
</figure>
"""


def rewrite(html: str) -> str:
    # Drop the README banner — the site has its own hero.
    html = re.sub(r'<p><img[^>]*banner\.jpg[^>]*></p>\s*', "", html)
    html = re.sub(r'<img[^>]*banner\.jpg[^>]*>', "", html)
    # Intra-repo doc links → clean local URLs.
    html = html.replace('href="docs/helpers.md"', 'href="/docs/helpers/"')
    html = html.replace('href="docs/configuration.md"', 'href="/docs/configuration/"')
    html = html.replace('href="docs/troubleshooting.md"', 'href="/docs/troubleshooting/"')
    html = html.replace('href="helpers.md"', 'href="/docs/helpers/"')
    html = html.replace('href="configuration.md"', 'href="/docs/configuration/"')
    html = html.replace('href="troubleshooting.md"', 'href="/docs/troubleshooting/"')
    html = html.replace('href="README.md"', 'href="/docs/"')
    html = html.replace('href="../README.md"', 'href="/docs/"')
    return html


def nav_for(active: str) -> str:
    rows = []
    for slug, _, _, label in PAGES:
        href = "/docs/" if slug == "" else f"/docs/{slug}/"
        cur = ' aria-current="page"' if slug == active else ""
        rows.append(f'      <a href="{href}"{cur}>{label}</a>')
    return "\n".join(rows)


def main():
    docs_out = OUT / "docs"
    docs_out.mkdir(parents=True, exist_ok=True)
    (docs_out / "docs.css").write_text(CSS)
    for slug, title, src, _ in PAGES:
        content = rewrite(md_to_html(src))
        if slug == "":
            # The overview opens with the system drawn, not just described:
            # inject the observation figure after the intro paragraphs,
            # before the first h2.
            content = content.replace("<h2>", OVERVIEW_DIAGRAM + "<h2>", 1)
        page = LAYOUT.format(
            title=title,
            canon="" if slug == "" else slug + "/",
            active_docs=' aria-current="page"',
            nav=nav_for(slug),
            content=content,
        )
        dest = docs_out if slug == "" else docs_out / slug
        dest.mkdir(parents=True, exist_ok=True)
        (dest / "index.html").write_text(page)
        print(f"built /docs/{slug + '/' if slug else ''}  ({len(page)} bytes)")


if __name__ == "__main__":
    main()
