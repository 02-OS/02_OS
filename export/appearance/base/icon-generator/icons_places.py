# Places & Folders SVG definitions for 02-OS

def get_places_icons():
    icons = {}

    def base_folder(badge):
        return f"""
  <!-- Back Folder Tab -->
  <path d="M 64 128 C 64 104, 80 88, 104 88 L 196 88 C 214 88, 228 98, 240 112 L 262 138 C 272 150, 286 158, 302 158 L 408 158 C 432 158, 448 174, 448 198 L 448 396 C 448 420, 432 436, 408 436 L 104 436 C 80 436, 64 420, 64 396 Z" fill="url(#f-back)" filter="url(#folder-shadow)"/>
  
  <!-- Inner White Paper Page -->
  <path d="M 96 140 C 96 126, 108 114, 122 114 L 390 114 C 404 114, 416 126, 416 140 L 416 260 L 96 260 Z" fill="#f8fafc" opacity="0.94"/>
  <line x1="130" y1="144" x2="230" y2="144" stroke="#cbd5e1" stroke-width="4" stroke-linecap="round"/>
  <line x1="130" y1="160" x2="350" y2="160" stroke="#cbd5e1" stroke-width="3" stroke-linecap="round"/>

  <!-- Front Pocket -->
  <g filter="url(#pocket-shadow)">
    <path d="M 52 196 C 52 176, 68 160, 88 160 L 424 160 C 444 160, 460 176, 460 196 L 460 396 C 460 422, 440 442, 414 442 L 98 442 C 72 442, 52 422, 52 396 Z" fill="url(#f-front)"/>
    <!-- Top Pocket Highlight Rim -->
    <path d="M 52 196 C 52 176, 68 160, 88 160 L 424 160 C 444 160, 460 176, 460 196" fill="none" stroke="url(#f-rim)" stroke-width="3"/>
  </g>

  <!-- Front Pocket Badge -->
  <g filter="url(#glyph-shadow)">
    {badge}
  </g>
        """

    defs_folder = """
    <linearGradient id="f-back" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#0284c7"/>
      <stop offset="100%" stop-color="#0369a1"/>
    </linearGradient>
    <linearGradient id="f-front" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#38bdf8"/>
      <stop offset="50%" stop-color="#0284c7"/>
      <stop offset="100%" stop-color="#075985"/>
    </linearGradient>
    <linearGradient id="f-rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.6"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.1"/>
    </linearGradient>
    """

    # 1. folder
    icons["folder"] = (defs_folder, base_folder(""))

    # 2. folder-documents
    badge_docs = """
    <rect x="216" y="240" width="80" height="110" rx="10" fill="#ffffff" opacity="0.9"/>
    <line x1="236" y1="264" x2="276" y2="264" stroke="#0284c7" stroke-width="5" stroke-linecap="round"/>
    <line x1="236" y1="284" x2="276" y2="284" stroke="#64748b" stroke-width="4" stroke-linecap="round"/>
    <line x1="236" y1="304" x2="266" y2="304" stroke="#64748b" stroke-width="4" stroke-linecap="round"/>
    <line x1="236" y1="324" x2="276" y2="324" stroke="#64748b" stroke-width="4" stroke-linecap="round"/>
    """
    icons["folder-documents"] = (defs_folder, base_folder(badge_docs))

    # 3. folder-download
    badge_dl = """
    <circle cx="256" cy="300" r="54" fill="#0369a1" opacity="0.5"/>
    <path d="M 256 250 L 256 325 M 224 295 L 256 325 L 288 295" fill="none" stroke="#ffffff" stroke-width="12" stroke-linecap="round" stroke-linejoin="round"/>
    <line x1="216" y1="345" x2="296" y2="345" stroke="#ffffff" stroke-width="10" stroke-linecap="round"/>
    """
    icons["folder-download"] = (defs_folder, base_folder(badge_dl))

    # 4. folder-music
    badge_music = """
    <circle cx="256" cy="300" r="54" fill="#0369a1" opacity="0.5"/>
    <circle cx="230" cy="326" r="16" fill="#ffffff"/>
    <circle cx="282" cy="310" r="16" fill="#ffffff"/>
    <line x1="242" y1="326" x2="242" y2="256" stroke="#ffffff" stroke-width="8"/>
    <line x1="294" y1="310" x2="294" y2="240" stroke="#ffffff" stroke-width="8"/>
    <polygon points="238,260 298,244 298,260 238,276" fill="#ffffff"/>
    """
    icons["folder-music"] = (defs_folder, base_folder(badge_music))

    # 5. folder-pictures
    badge_pics = """
    <rect x="206" y="246" width="100" height="84" rx="10" fill="#ffffff" opacity="0.9"/>
    <circle cx="234" cy="270" r="8" fill="#f59e0b"/>
    <polygon points="206,326 246,280 276,310 290,296 306,326" fill="#0284c7"/>
    """
    icons["folder-pictures"] = (defs_folder, base_folder(badge_pics))

    # 6. folder-videos
    badge_videos = """
    <rect x="206" y="250" width="100" height="80" rx="12" fill="#0f172a" stroke="#ffffff" stroke-width="6"/>
    <polygon points="244,272 278,290 244,308" fill="#ffffff"/>
    """
    icons["folder-videos"] = (defs_folder, base_folder(badge_videos))

    # 7. folder-home
    badge_home = """
    <path d="M 216 340 L 216 290 L 256 250 L 296 290 L 296 340 Z" fill="#ffffff"/>
    <path d="M 200 296 L 256 244 L 312 296" fill="none" stroke="#ffffff" stroke-width="8" stroke-linecap="round"/>
    <rect x="244" y="306" width="24" height="34" rx="4" fill="#0284c7"/>
    """
    icons["folder-home"] = (defs_folder, base_folder(badge_home))

    # 8. folder-desktop
    badge_desktop = """
    <rect x="206" y="246" width="100" height="70" rx="10" fill="#ffffff" opacity="0.9"/>
    <rect x="214" y="254" width="84" height="50" rx="6" fill="#0284c7"/>
    <line x1="256" y1="316" x2="256" y2="336" stroke="#ffffff" stroke-width="8" stroke-linecap="round"/>
    <line x1="236" y1="336" x2="276" y2="336" stroke="#ffffff" stroke-width="8" stroke-linecap="round"/>
    """
    icons["folder-desktop"] = (defs_folder, base_folder(badge_desktop))

    # 9. folder-publicshare
    badge_public = """
    <circle cx="256" cy="300" r="54" fill="#0369a1" opacity="0.5"/>
    <circle cx="256" cy="276" r="16" fill="#ffffff"/>
    <path d="M 224 330 C 224 310, 240 300, 256 300 C 272 300, 288 310, 288 330" fill="none" stroke="#ffffff" stroke-width="8" stroke-linecap="round"/>
    """
    icons["folder-publicshare"] = (defs_folder, base_folder(badge_public))

    # 10. folder-remote
    badge_remote = """
    <circle cx="256" cy="300" r="44" fill="none" stroke="#ffffff" stroke-width="8"/>
    <ellipse cx="256" cy="300" rx="44" ry="18" fill="none" stroke="#ffffff" stroke-width="6"/>
    <line x1="256" y1="256" x2="256" y2="344" stroke="#ffffff" stroke-width="6"/>
    """
    icons["folder-remote"] = (defs_folder, base_folder(badge_remote))

    # 11. folder-templates
    badge_templates = """
    <polygon points="216,340 296,260 296,340" fill="none" stroke="#ffffff" stroke-width="8"/>
    <polygon points="244,324 276,292 276,324" fill="#ffffff" opacity="0.7"/>
    """
    icons["folder-templates"] = (defs_folder, base_folder(badge_templates))

    # 12. folder-open
    art_open = """
  <!-- Back Folder Body -->
  <path d="M 64 128 C 64 104, 80 88, 104 88 L 196 88 C 214 88, 228 98, 240 112 L 262 138 C 272 150, 286 158, 302 158 L 408 158 C 432 158, 448 174, 448 198 L 448 396 C 448 420, 432 436, 408 436 L 104 436 C 80 436, 64 420, 64 396 Z" fill="url(#f-back)" filter="url(#folder-shadow)"/>
  
  <!-- Multiple Document Pages Peeking Out -->
  <path d="M 120 120 L 392 120 L 392 300 L 120 300 Z" fill="#f8fafc" opacity="0.9"/>
  <path d="M 100 140 L 372 140 L 372 300 L 100 300 Z" fill="#e2e8f0" opacity="0.9"/>
  <path d="M 80 160 L 352 160 L 352 300 L 80 300 Z" fill="#ffffff" opacity="0.95"/>

  <!-- Angled Open Pocket -->
  <g filter="url(#pocket-shadow)">
    <polygon points="36,250 476,220 440,436 72,436" fill="url(#f-front)"/>
    <line x1="36" y1="250" x2="476" y2="220" stroke="url(#f-rim)" stroke-width="4"/>
  </g>
    """
    icons["folder-open"] = (defs_folder, art_open)

    # 13. user-trash (Empty Trash)
    defs_trash = """
    <linearGradient id="bg-trash" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#334155"/>
      <stop offset="50%" stop-color="#1e293b"/>
      <stop offset="100%" stop-color="#0f172a"/>
    </linearGradient>
    <linearGradient id="can-mesh" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#f1f5f9" stop-opacity="0.8"/>
      <stop offset="100%" stop-color="#94a3b8" stop-opacity="0.4"/>
    </linearGradient>
    """
    art_trash = """
  <!-- Base Squircle with Shadow -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="url(#bg-trash)" filter="url(#shadow)"/>
  <path d="M 36 140 C 36 82, 82 36, 140 36 L 372 36 C 430 36, 476 82, 476 140 L 476 210 C 360 240, 150 240, 36 210 Z" fill="url(#top-gloss)"/>
  <g filter="url(#glyph-shadow)">
    <!-- Trash Can Lid Top Rim -->
    <ellipse cx="256" cy="140" rx="120" ry="24" fill="none" stroke="#e2e8f0" stroke-width="12"/>
    <ellipse cx="256" cy="140" rx="40" ry="10" fill="none" stroke="#e2e8f0" stroke-width="8"/>

    <!-- Slanted Can Body -->
    <polygon points="146,140 366,140 336,400 176,400" fill="url(#can-mesh)"/>
    <line x1="176" y1="400" x2="336" y2="400" stroke="#e2e8f0" stroke-width="12" stroke-linecap="round"/>

    <!-- Vertical Slats -->
    <line x1="200" y1="150" x2="208" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
    <line x1="236" y1="152" x2="238" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
    <line x1="276" y1="152" x2="274" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
    <line x1="312" y1="150" x2="304" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
  </g>
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="none" stroke="url(#specular-rim)" stroke-width="2.5"/>
    """
    icons["user-trash"] = (defs_trash, art_trash)

    # 14. user-trash-full (Full Trash)
    art_trash_full = """
  <!-- Base Squircle with Shadow -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="url(#bg-trash)" filter="url(#shadow)"/>
  <path d="M 36 140 C 36 82, 82 36, 140 36 L 372 36 C 430 36, 476 82, 476 140 L 476 210 C 360 240, 150 240, 36 210 Z" fill="url(#top-gloss)"/>
  <g filter="url(#glyph-shadow)">
    <!-- Crumpled Paper Overflowing -->
    <polygon points="200,120 230,80 260,110 240,130" fill="#38bdf8"/>
    <polygon points="250,90 280,60 310,90 280,120" fill="#f8fafc"/>
    <polygon points="270,110 320,100 330,130 290,140" fill="#f43f5e"/>

    <!-- Trash Can Lid Top Rim -->
    <ellipse cx="256" cy="140" rx="120" ry="24" fill="none" stroke="#e2e8f0" stroke-width="12"/>
    <ellipse cx="256" cy="140" rx="40" ry="10" fill="none" stroke="#e2e8f0" stroke-width="8"/>

    <!-- Slanted Can Body -->
    <polygon points="146,140 366,140 336,400 176,400" fill="url(#can-mesh)"/>
    <line x1="176" y1="400" x2="336" y2="400" stroke="#e2e8f0" stroke-width="12" stroke-linecap="round"/>

    <!-- Vertical Slats -->
    <line x1="200" y1="150" x2="208" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
    <line x1="236" y1="152" x2="238" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
    <line x1="276" y1="152" x2="274" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
    <line x1="312" y1="150" x2="304" y2="390" stroke="#0f172a" stroke-width="6" opacity="0.3"/>
  </g>
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="none" stroke="url(#specular-rim)" stroke-width="2.5"/>
    """
    icons["user-trash-full"] = (defs_trash, art_trash_full)

    return icons

print("icons_places module updated.")
