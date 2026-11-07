# Devices, Categories, and Mimetypes SVG definitions for 02-OS

def get_devices_icons():
    icons = {}

    # 1. drive-harddisk (SSD / Hard Drive)
    defs_hdd = """
    <linearGradient id="bg-hdd" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#334155"/>
      <stop offset="50%" stop-color="#1e293b"/>
      <stop offset="100%" stop-color="#0f172a"/>
    </linearGradient>
    <linearGradient id="drive-metal" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#f8fafc"/>
      <stop offset="100%" stop-color="#94a3b8"/>
    </linearGradient>
    """
    art_hdd = """
    <!-- Aluminum SSD Enclosure -->
    <rect x="110" y="110" width="292" height="292" rx="32" ry="32" fill="url(#drive-metal)" stroke="#475569" stroke-width="4"/>
    
    <!-- Cooling Fins -->
    <rect x="140" y="160" width="232" height="12" rx="6" fill="#1e293b"/>
    <rect x="140" y="190" width="232" height="12" rx="6" fill="#1e293b"/>
    <rect x="140" y="220" width="232" height="12" rx="6" fill="#1e293b"/>

    <!-- Capacity Plate -->
    <rect x="140" y="270" width="232" height="90" rx="16" fill="#0f172a"/>
    <circle cx="176" cy="315" r="14" fill="#38bdf8"/>
    <line x1="210" y1="305" x2="330" y2="305" stroke="#f8fafc" stroke-width="6" stroke-linecap="round"/>
    <line x1="210" y1="325" x2="280" y2="325" stroke="#38bdf8" stroke-width="6" stroke-linecap="round"/>
    """
    icons["drive-harddisk"] = (defs_hdd, "bg-hdd", art_hdd)

    # 2. drive-removable-media (USB Flash Drive)
    defs_usb = """
    <linearGradient id="bg-usb" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#0369a1"/>
      <stop offset="100%" stop-color="#0284c7"/>
    </linearGradient>
    """
    art_usb = """
    <!-- USB Type-C Connector -->
    <rect x="216" y="90" width="80" height="70" rx="12" fill="#cbd5e1" stroke="#64748b" stroke-width="4"/>
    <line x1="236" y1="120" x2="276" y2="120" stroke="#0f172a" stroke-width="6" stroke-linecap="round"/>

    <!-- Drive Body -->
    <rect x="186" y="150" width="140" height="250" rx="28" ry="28" fill="#0f172a" stroke="#ffffff" stroke-width="6"/>
    <circle cx="256" cy="220" r="16" fill="#38bdf8"/>
    <line x1="256" y1="270" x2="256" y2="360" stroke="#64748b" stroke-width="8" stroke-linecap="round"/>
    """
    icons["drive-removable-media"] = (defs_usb, "bg-usb", art_usb)

    # 3. media-optical (CD / DVD)
    defs_cd = """
    <linearGradient id="bg-cd" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#312e81"/>
      <stop offset="50%" stop-color="#4c1d95"/>
      <stop offset="100%" stop-color="#831843"/>
    </linearGradient>
    """
    art_cd = """
    <!-- Optical Disc Outer -->
    <circle cx="256" cy="256" r="140" fill="#e2e8f0" stroke="#94a3b8" stroke-width="4"/>
    <!-- Rainbow Specular Sheen -->
    <circle cx="256" cy="256" r="130" fill="none" stroke="#38bdf8" stroke-width="24" opacity="0.4"/>
    <circle cx="256" cy="256" r="106" fill="none" stroke="#ec4899" stroke-width="24" opacity="0.4"/>
    <circle cx="256" cy="256" r="82" fill="none" stroke="#f59e0b" stroke-width="24" opacity="0.4"/>
    <!-- Clear Hub -->
    <circle cx="256" cy="256" r="54" fill="#ffffff" opacity="0.6"/>
    <!-- Spindle Hole -->
    <circle cx="256" cy="256" r="28" fill="#312e81"/>
    """
    icons["media-optical"] = (defs_cd, "bg-cd", art_cd)

    # 4. computer
    defs_comp = """
    <linearGradient id="bg-comp" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#1e293b"/>
      <stop offset="100%" stop-color="#0f172a"/>
    </linearGradient>
    """
    art_comp = """
    <!-- Monitor Display -->
    <rect x="96" y="110" width="320" height="210" rx="20" ry="20" fill="#0284c7" stroke="#ffffff" stroke-width="8"/>
    <!-- Stand Neck -->
    <rect x="236" y="320" width="40" height="50" fill="#94a3b8"/>
    <!-- Stand Base -->
    <path d="M 186 370 L 326 370" stroke="#ffffff" stroke-width="14" stroke-linecap="round"/>
    <!-- Screen Glint -->
    <polygon points="106,120 200,120 106,214" fill="#ffffff" opacity="0.3"/>
    """
    icons["computer"] = (defs_comp, "bg-comp", art_comp)

    return icons

def get_categories_icons():
    icons = {}

    defs_cat = """
    <linearGradient id="bg-cat" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#1e293b"/>
      <stop offset="100%" stop-color="#0f172a"/>
    </linearGradient>
    """

    # 1. applications-accessories
    art_acc = """
    <g transform="translate(256, 256) rotate(-30) translate(-256, -256)">
      <rect x="236" y="110" width="40" height="292" rx="20" fill="#ef4444"/>
      <circle cx="256" cy="150" r="10" fill="#ffffff"/>
      <!-- Compass Legs -->
      <line x1="256" y1="200" x2="160" y2="370" stroke="#ffffff" stroke-width="14" stroke-linecap="round"/>
      <line x1="256" y1="200" x2="352" y2="370" stroke="#ffffff" stroke-width="14" stroke-linecap="round"/>
    </g>
    """
    icons["applications-accessories"] = (defs_cat, "bg-cat", art_acc)

    # 2. applications-development
    art_dev = """
    <!-- Code Brackets -->
    <path d="M 170 180 L 100 256 L 170 332" fill="none" stroke="#38bdf8" stroke-width="24" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M 342 180 L 412 256 L 342 332" fill="none" stroke="#38bdf8" stroke-width="24" stroke-linecap="round" stroke-linejoin="round"/>
    <line x1="280" y1="160" x2="232" y2="352" stroke="#f43f5e" stroke-width="24" stroke-linecap="round"/>
    """
    icons["applications-development"] = (defs_cat, "bg-cat", art_dev)

    # 3. applications-games
    art_games = """
    <!-- Gamepad Controller -->
    <path d="M 160 170 C 120 170, 80 200, 90 280 C 100 350, 150 360, 180 320 L 210 280 L 302 280 L 332 320 C 362 360, 412 350, 422 280 C 432 200, 392 170, 352 170 Z" fill="#6366f1"/>
    <!-- D-pad -->
    <path d="M 160 210 L 160 270 M 130 240 L 190 240" stroke="#ffffff" stroke-width="12" stroke-linecap="round"/>
    <!-- Action Buttons -->
    <circle cx="340" cy="226" r="10" fill="#f43f5e"/>
    <circle cx="366" cy="252" r="10" fill="#38bdf8"/>
    <circle cx="314" cy="252" r="10" fill="#facc15"/>
    <circle cx="340" cy="278" r="10" fill="#22c55e"/>
    """
    icons["applications-games"] = (defs_cat, "bg-cat", art_games)

    # 4. applications-graphics
    art_gfx = """
    <!-- Artist Palette -->
    <path d="M 256 120 C 160 120, 100 180, 100 270 C 100 350, 170 400, 240 400 C 260 400, 280 380, 280 360 C 280 340, 270 330, 290 320 C 310 310, 330 330, 360 330 C 400 330, 420 280, 412 230 C 400 160, 340 120, 256 120 Z" fill="#d97706"/>
    <!-- Paint Dots -->
    <circle cx="160" cy="200" r="18" fill="#ef4444"/>
    <circle cx="220" cy="170" r="18" fill="#3b82f6"/>
    <circle cx="290" cy="170" r="18" fill="#10b981"/>
    <circle cx="350" cy="210" r="18" fill="#facc15"/>
    <!-- Thumb Hole -->
    <circle cx="340" cy="270" r="18" fill="#1e293b"/>
    """
    icons["applications-graphics"] = (defs_cat, "bg-cat", art_gfx)

    # 5. applications-internet
    art_net = """
    <!-- Globe -->
    <circle cx="256" cy="256" r="120" fill="#0284c7" stroke="#ffffff" stroke-width="8"/>
    <ellipse cx="256" cy="256" rx="60" ry="120" fill="none" stroke="#ffffff" stroke-width="8"/>
    <line x1="136" y1="256" x2="376" y2="256" stroke="#ffffff" stroke-width="8"/>
    <line x1="156" y1="190" x2="356" y2="190" stroke="#ffffff" stroke-width="6"/>
    <line x1="156" y1="322" x2="356" y2="322" stroke="#ffffff" stroke-width="6"/>
    """
    icons["applications-internet"] = (defs_cat, "bg-cat", art_net)

    # 6. applications-multimedia
    art_multi = """
    <!-- Film Reel Clapper & Note -->
    <rect x="136" y="150" width="240" height="180" rx="16" fill="#0f172a" stroke="#ffffff" stroke-width="8"/>
    <polygon points="230,200 300,240 230,280" fill="#38bdf8"/>
    <circle cx="340" cy="350" r="22" fill="#a855f7"/>
    <line x1="358" y1="350" x2="358" y2="280" stroke="#a855f7" stroke-width="8"/>
    """
    icons["applications-multimedia"] = (defs_cat, "bg-cat", art_multi)

    # 7. applications-office
    art_office = """
    <!-- Briefcase -->
    <rect x="120" y="190" width="272" height="180" rx="20" fill="#92400e" stroke="#fcd34d" stroke-width="6"/>
    <path d="M 210 190 L 210 140 C 210 128, 220 120, 232 120 L 280 120 C 292 120, 302 128, 302 140 L 302 190" fill="none" stroke="#fcd34d" stroke-width="12"/>
    <line x1="120" y1="256" x2="392" y2="256" stroke="#fcd34d" stroke-width="6"/>
    <rect x="236" y="240" width="40" height="32" rx="6" fill="#fcd34d"/>
    """
    icons["applications-office"] = (defs_cat, "bg-cat", art_office)

    # 8. applications-system
    art_sys = """
    <!-- Gauge and Wrench -->
    <circle cx="256" cy="256" r="120" fill="#0f172a" stroke="#ffffff" stroke-width="12"/>
    <path d="M 180 300 C 160 250, 180 190, 230 160 C 280 130, 340 150, 360 200" fill="none" stroke="#38bdf8" stroke-width="16" stroke-linecap="round"/>
    <line x1="256" y1="256" x2="310" y2="180" stroke="#ef4444" stroke-width="10" stroke-linecap="round"/>
    <circle cx="256" cy="256" r="16" fill="#ffffff"/>
    """
    icons["applications-system"] = (defs_cat, "bg-cat", art_sys)

    # 9. applications-utilities
    art_util = """
    <!-- Crossed Screwdriver & Wrench -->
    <g transform="translate(256, 256)">
      <!-- Wrench -->
      <line x1="-90" y1="-90" x2="90" y2="90" stroke="#ffffff" stroke-width="24" stroke-linecap="round"/>
      <circle cx="-90" cy="-90" r="30" fill="none" stroke="#ffffff" stroke-width="16"/>
      <!-- Screwdriver -->
      <line x1="90" y1="-90" x2="-90" y2="90" stroke="#f59e0b" stroke-width="20" stroke-linecap="round"/>
      <rect x="40" y="-120" width="40" height="50" rx="8" fill="#3b82f6" transform="rotate(45)"/>
    </g>
    """
    icons["applications-utilities"] = (defs_cat, "bg-cat", art_util)

    # 10. preferences-desktop
    art_pdesk = """
    <!-- Monitor with Wallpaper Horizon -->
    <rect x="110" y="120" width="292" height="200" rx="16" fill="#0f172a" stroke="#ffffff" stroke-width="8"/>
    <polygon points="120,310 200,220 260,280 320,200 392,310" fill="#ec4899"/>
    <circle cx="340" cy="170" r="18" fill="#facc15"/>
    <!-- Stand -->
    <line x1="256" y1="320" x2="256" y2="370" stroke="#ffffff" stroke-width="14"/>
    <line x1="200" y1="370" x2="312" y2="370" stroke="#ffffff" stroke-width="14" stroke-linecap="round"/>
    """
    icons["preferences-desktop"] = (defs_cat, "bg-cat", art_pdesk)

    return icons

def get_mimetypes_icons():
    icons = {}

    def doc_template(ribbon_color, badge):
        defs = f"""
        <linearGradient id="doc-bg" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#ffffff"/>
          <stop offset="100%" stop-color="#f1f5f9"/>
        </linearGradient>
        <linearGradient id="fold-grad" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stop-color="#cbd5e1"/>
          <stop offset="100%" stop-color="#94a3b8"/>
        </linearGradient>
        """
        body = f"""
  <!-- Document Page with Folded Corner -->
  <path d="M 120 70 L 320 70 L 392 142 L 392 442 L 120 442 Z" fill="url(#doc-bg)" filter="url(#shadow)"/>
  <path d="M 320 70 L 320 142 L 392 142 Z" fill="url(#fold-grad)"/>

  <!-- Accent Bottom Ribbon -->
  <rect x="120" y="380" width="272" height="62" fill="{ribbon_color}"/>

  <!-- Badge in center -->
  <g filter="url(#glyph-shadow)">
    {badge}
  </g>
        """
        return defs, body

    # 1. text-plain
    b_txt = """
    <line x1="160" y1="180" x2="350" y2="180" stroke="#64748b" stroke-width="10" stroke-linecap="round"/>
    <line x1="160" y1="220" x2="350" y2="220" stroke="#64748b" stroke-width="10" stroke-linecap="round"/>
    <line x1="160" y1="260" x2="350" y2="260" stroke="#64748b" stroke-width="10" stroke-linecap="round"/>
    <line x1="160" y1="300" x2="270" y2="300" stroke="#64748b" stroke-width="10" stroke-linecap="round"/>
    """
    d, b = doc_template("#64748b", b_txt)
    icons["text-plain"] = (d, b)
    icons["text-x-generic"] = (d, b)

    # 2. text-x-script (Shell Script)
    b_sh = """
    <text x="256" y="270" fill="#0284c7" font-family="monospace" font-size="56" font-weight="bold" text-anchor="middle">&gt;_</text>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">SHELL</text>
    """
    d, b = doc_template("#0284c7", b_sh)
    icons["text-x-script"] = (d, b)

    # 3. text-x-python
    b_py = """
    <circle cx="256" cy="240" r="46" fill="#3b82f6"/>
    <circle cx="256" cy="270" r="46" fill="#eab308"/>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">PYTHON</text>
    """
    d, b = doc_template("#eab308", b_py)
    icons["text-x-python"] = (d, b)

    # 4. application-pdf
    b_pdf = """
    <text x="256" y="270" fill="#ef4444" font-family="Inter, sans-serif" font-size="64" font-weight="900" text-anchor="middle">PDF</text>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">DOCUMENT</text>
    """
    d, b = doc_template("#dc2626", b_pdf)
    icons["application-pdf"] = (d, b)

    # 5. application-x-executable
    b_bin = """
    <circle cx="256" cy="250" r="44" fill="#6366f1"/>
    <text x="256" y="262" fill="#ffffff" font-family="monospace" font-size="34" font-weight="bold" text-anchor="middle">0101</text>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">BIN</text>
    """
    d, b = doc_template("#4f46e5", b_bin)
    icons["application-x-executable"] = (d, b)

    # 6. application-zip
    b_zip = """
    <line x1="256" y1="160" x2="256" y2="340" stroke="#f59e0b" stroke-width="16" stroke-dasharray="16,10"/>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">ARCHIVE</text>
    """
    d, b = doc_template("#d97706", b_zip)
    icons["application-zip"] = (d, b)

    # 7. image-x-generic
    b_img = """
    <circle cx="220" cy="220" r="16" fill="#f59e0b"/>
    <polygon points="180,310 240,230 290,290 320,260 340,310" fill="#ec4899"/>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">IMAGE</text>
    """
    d, b = doc_template("#db2777", b_img)
    icons["image-x-generic"] = (d, b)

    # 8. audio-x-generic
    b_audio = """
    <circle cx="236" cy="280" r="18" fill="#8b5cf6"/>
    <circle cx="286" cy="260" r="18" fill="#8b5cf6"/>
    <line x1="252" y1="280" x2="252" y2="190" stroke="#8b5cf6" stroke-width="8"/>
    <line x1="302" y1="260" x2="302" y2="170" stroke="#8b5cf6" stroke-width="8"/>
    <polygon points="248,194 306,174 306,194 248,214" fill="#8b5cf6"/>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">AUDIO</text>
    """
    d, b = doc_template("#7c3aed", b_audio)
    icons["audio-x-generic"] = (d, b)

    # 9. video-x-generic
    b_video = """
    <rect x="196" y="210" width="120" height="90" rx="14" fill="#ef4444"/>
    <polygon points="244,235 278,255 244,275" fill="#ffffff"/>
    <text x="256" y="426" fill="#ffffff" font-family="Inter, sans-serif" font-size="28" font-weight="bold" text-anchor="middle">VIDEO</text>
    """
    d, b = doc_template("#dc2626", b_video)
    icons["video-x-generic"] = (d, b)

    return icons

print("icons_devices_categories module ready.")
