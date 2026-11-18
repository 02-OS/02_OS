#!/usr/bin/env python3
import os
import re

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, "..", ".."))
ICONS_DIR = os.path.join(REPO_ROOT, "profile", "airootfs", "usr", "share", "icons", "02-OS", "scalable")
HTML_OUT = os.path.join(REPO_ROOT, "02-OS-icons-preview.html")

ID_ATTR_RE = re.compile(r"""(\bid\s*=\s*)(["'])([^"']+)\2""")
REF_RE = re.compile(
    r"""(?:url\(\s*["']?#|(?:xlink:)?href\s*=\s*["']#)(?P<id>[^"'#)\s]+)"""
)


def svg_id_prefix(category, name, index):
    raw = f"{category}-{name}-{index}"
    safe = re.sub(r"[^A-Za-z0-9_-]", "-", raw).strip("-")
    if not safe or safe[0].isdigit():
        safe = f"n{safe}"
    return f"icon-{safe}-"


def prefix_svg_ids(svg, prefix):
    """Prefix ids in one inline SVG and rewrite its local fragment references."""
    mapping = {}
    for match in ID_ATTR_RE.finditer(svg):
        mapping.setdefault(match.group(3), f"{prefix}{match.group(3)}")

    def repl_id(match):
        quote = match.group(2)
        old = match.group(3)
        return f"{match.group(1)}{quote}{mapping[old]}{quote}"

    svg = ID_ATTR_RE.sub(repl_id, svg)

    def repl_ref(match):
        old = match.group("id")
        new = mapping.get(old)
        if new is None:
            return match.group(0)
        return match.group(0).replace(f"#{old}", f"#{new}", 1)

    return REF_RE.sub(repl_ref, svg)


def build_gallery():
    categories = ["apps", "places", "devices", "categories", "mimetypes", "status", "actions"]
    sections = []
    card_index = 0

    for cat in categories:
        cat_path = os.path.join(ICONS_DIR, cat)
        if not os.path.exists(cat_path):
            continue

        files = sorted([f for f in os.listdir(cat_path) if f.endswith(".svg") and not os.path.islink(os.path.join(cat_path, f))])
        symlinks = sorted([f for f in os.listdir(cat_path) if f.endswith(".svg") and os.path.islink(os.path.join(cat_path, f))])

        cards = []
        for f in files:
            svg_path = os.path.join(cat_path, f)
            with open(svg_path, 'r') as s:
                svg_data = s.read()
            name_for_id = f[:-4]
            svg_data = prefix_svg_ids(svg_data, svg_id_prefix(cat, name_for_id, card_index))
            card_index += 1

            # Extract title without .svg
            name = f[:-4]
            # Find aliases
            aliases = [s[:-4] for s in symlinks if os.readlink(os.path.join(cat_path, s)) == f]
            alias_badges = "".join([f'<span class="alias-tag">{a}</span>' for a in aliases[:3]])
            if len(aliases) > 3:
                alias_badges += f'<span class="alias-tag">+{len(aliases)-3} more</span>'

            card = f"""
            <div class="icon-card">
              <div class="icon-preview">
                {svg_data}
              </div>
              <div class="icon-name">{name}</div>
              <div class="aliases">{alias_badges}</div>
            </div>
            """
            cards.append(card)

        section = f"""
        <section class="category-section">
          <h2 class="category-title">{cat.capitalize()} <span class="count-badge">{len(files)} icons ({len(symlinks)} aliases)</span></h2>
          <div class="icon-grid">
            {"".join(cards)}
          </div>
        </section>
        """
        sections.append(section)

    html = f"""<!DOCTYPE html>
    <html lang="en">
    <head>
      <meta charset="UTF-8">
      <title>02_OS — Glassmorphic Vector Icon Pack Preview</title>
      <style>
        :root {{
          --bg: #0d0f14;
          --card-bg: rgba(26, 29, 38, 0.7);
          --card-border: rgba(255, 255, 255, 0.08);
          --text-main: #f1f5f9;
          --text-muted: #94a3b8;
          --accent: #38bdf8;
          --accent-glow: rgba(56, 189, 248, 0.2);
        }}
        * {{
          box-sizing: border-box;
          margin: 0;
          padding: 0;
        }}
        body {{
          background-color: var(--bg);
          background-image:
            radial-gradient(at 0% 0%, rgba(56, 189, 248, 0.12) 0px, transparent 50%),
            radial-gradient(at 100% 0%, rgba(168, 85, 247, 0.12) 0px, transparent 50%),
            radial-gradient(at 50% 100%, rgba(14, 165, 233, 0.08) 0px, transparent 50%);
          color: var(--text-main);
          font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
          min-height: 100vh;
          padding: 40px 24px 80px;
        }}
        .header {{
          max-width: 1200px;
          margin: 0 auto 48px;
          text-align: center;
        }}
        .badge {{
          display: inline-block;
          padding: 6px 14px;
          background: rgba(56, 189, 248, 0.15);
          border: 1px solid rgba(56, 189, 248, 0.35);
          border-radius: 9999px;
          font-size: 12px;
          font-weight: 600;
          letter-spacing: 0.5px;
          color: #38bdf8;
          margin-bottom: 16px;
          text-transform: uppercase;
        }}
        h1 {{
          font-size: 42px;
          font-weight: 800;
          letter-spacing: -1px;
          margin-bottom: 12px;
          background: linear-gradient(135deg, #ffffff 0%, #94a3b8 100%);
          -webkit-background-clip: text;
          -webkit-text-fill-color: transparent;
        }}
        .subtitle {{
          font-size: 16px;
          color: var(--text-muted);
          max-width: 640px;
          margin: 0 auto;
          line-height: 1.6;
        }}
        .container {{
          max-width: 1280px;
          margin: 0 auto;
        }}
        .category-section {{
          margin-bottom: 56px;
        }}
        .category-title {{
          font-size: 22px;
          font-weight: 700;
          margin-bottom: 24px;
          display: flex;
          align-items: center;
          gap: 12px;
          padding-bottom: 10px;
          border-bottom: 1px solid rgba(255, 255, 255, 0.08);
        }}
        .count-badge {{
          font-size: 13px;
          font-weight: 500;
          color: var(--text-muted);
          background: rgba(255, 255, 255, 0.06);
          padding: 3px 10px;
          border-radius: 999px;
        }}
        .icon-grid {{
          display: grid;
          grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
          gap: 20px;
        }}
        .icon-card {{
          background: var(--card-bg);
          backdrop-filter: blur(16px);
          -webkit-backdrop-filter: blur(16px);
          border: 1px solid var(--card-border);
          border-radius: 20px;
          padding: 20px 14px 16px;
          display: flex;
          flex-direction: column;
          align-items: center;
          text-align: center;
          transition: all 0.25s cubic-bezier(0.16, 1, 0.3, 1);
        }}
        .icon-card:hover {{
          transform: translateY(-6px);
          border-color: rgba(56, 189, 248, 0.4);
          box-shadow: 0 16px 32px rgba(0, 0, 0, 0.45), 0 0 20px var(--accent-glow);
        }}
        .icon-preview {{
          width: 88px;
          height: 88px;
          margin-bottom: 14px;
          display: flex;
          align-items: center;
          justify-content: center;
          transition: transform 0.2s ease;
        }}
        .icon-card:hover .icon-preview {{
          transform: scale(1.06);
        }}
        .icon-preview svg {{
          width: 100%;
          height: 100%;
          overflow: visible;
        }}
        .icon-name {{
          font-size: 13px;
          font-weight: 600;
          color: var(--text-main);
          margin-bottom: 8px;
          word-break: break-word;
          font-family: 'JetBrains Mono', monospace;
        }}
        .aliases {{
          display: flex;
          flex-wrap: wrap;
          justify-content: center;
          gap: 4px;
        }}
        .alias-tag {{
          font-size: 10px;
          background: rgba(255, 255, 255, 0.05);
          border: 1px solid rgba(255, 255, 255, 0.08);
          color: var(--text-muted);
          padding: 2px 6px;
          border-radius: 6px;
          font-family: 'JetBrains Mono', monospace;
        }}
        .toolbar {{
          max-width: 720px;
          margin: 24px auto 0;
          display: flex;
          gap: 12px;
          justify-content: center;
          flex-wrap: wrap;
        }}
        .search-input, .category-filter {{
          background: rgba(26, 29, 38, 0.85);
          border: 1px solid rgba(255, 255, 255, 0.14);
          color: var(--text-main);
          border-radius: 12px;
          padding: 10px 14px;
          font-size: 14px;
          outline: none;
        }}
        .search-input {{
          flex: 1 1 280px;
        }}
      </style>
    </head>
    <body>
      <div class="header">
        <div class="badge">02_OS Design System</div>
        <h1>02-OS Glassmorphic Icon Pack</h1>
        <p class="subtitle">
          A bespoke, continuous-squircle vector icon system engineered for 02_OS.
          Crafted with multi-layer depth, frosted glass sheen, specular edge rim lighting, and vibrant gradients.
        </p>
        <div class="toolbar">
          <input id="gallery-search-input" class="search-input" type="search" placeholder="Search icons or aliases..." aria-label="Search icons">
          <select id="gallery-category-select" class="category-filter" aria-label="Filter by category">
            <option value="all">All categories</option>
            <option value="apps">Apps</option>
            <option value="places">Places</option>
            <option value="devices">Devices</option>
            <option value="categories">Categories</option>
            <option value="mimetypes">Mimetypes</option>
            <option value="status">Status</option>
            <option value="actions">Actions</option>
          </select>
        </div>
      </div>
      <div class="container">
        {"".join(sections)}
      </div>
      <script>
        (() => {{
          const searchInput = document.getElementById('gallery-search-input');
          const categorySelect = document.getElementById('gallery-category-select');
          const sections = Array.from(document.querySelectorAll('.category-section'));
          function applyFilter() {{
            const query = (searchInput?.value || '').trim().toLowerCase();
            const selectedCat = (categorySelect?.value || 'all').toLowerCase();
            for (const section of sections) {{
              const titleText = (section.querySelector('.category-title')?.textContent || '').trim().toLowerCase();
              const catMatch = selectedCat === 'all' || titleText.startsWith(selectedCat);
              let visibleCount = 0;
              for (const card of section.querySelectorAll('.icon-card')) {{
                const name = (card.querySelector('.icon-name')?.textContent || '').toLowerCase();
                const aliases = Array.from(card.querySelectorAll('.alias-tag')).map(el => el.textContent.toLowerCase()).join(' ');
                const matches = !query || name.includes(query) || aliases.includes(query);
                card.style.display = (catMatch && matches) ? '' : 'none';
                if (catMatch && matches) visibleCount++;
              }}
              section.style.display = (catMatch && visibleCount > 0) ? '' : 'none';
            }}
          }}
          searchInput?.addEventListener('input', applyFilter);
          categorySelect?.addEventListener('change', applyFilter);
        }})();
      </script>
    </body>
    </html>
    """

    return "\n".join(line.rstrip() for line in html.split("\n"))


def main():
    html = build_gallery()
    with open(HTML_OUT, "w") as f:
        f.write(html)
    print(f"Generated HTML preview gallery at: {HTML_OUT}")


if __name__ == "__main__":
    main()
