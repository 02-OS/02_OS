# Apps SVG definitions for 02-OS

def get_app_icons():
    icons = {}

    # 1. 02os-menu (02_OS Signature Monogram)
    defs_02 = """
    <linearGradient id="bg-02" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#090d16"/>
      <stop offset="50%" stop-color="#111827"/>
      <stop offset="100%" stop-color="#1e1b4b"/>
    </linearGradient>
    <linearGradient id="neon-cyan" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#38bdf8"/>
      <stop offset="100%" stop-color="#0284c7"/>
    </linearGradient>
    <linearGradient id="neon-violet" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#a855f7"/>
      <stop offset="100%" stop-color="#6366f1"/>
    </linearGradient>
    <linearGradient id="arch-glow" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#38bdf8" stop-opacity="0.6"/>
      <stop offset="100%" stop-color="#38bdf8" stop-opacity="0.0"/>
    </linearGradient>
    """
    art_02 = """
    <!-- Background Arch Chevron Outline -->
    <path d="M 256 100 L 370 330 L 320 330 L 290 270 L 222 270 L 192 330 L 142 330 Z" fill="url(#arch-glow)" opacity="0.4"/>
    
    <!-- Stylized "0" -->
    <ellipse cx="188" cy="256" rx="55" ry="85" fill="none" stroke="url(#neon-cyan)" stroke-width="26" stroke-linecap="round"/>
    <!-- Inner 0 glow pill -->
    <ellipse cx="188" cy="256" rx="28" ry="54" fill="#0284c7" opacity="0.25"/>

    <!-- Stylized "2" -->
    <path d="M 276 200 C 276 168, 305 152, 335 152 C 370 152, 396 176, 396 210 C 396 250, 345 285, 280 342 L 400 342" 
          fill="none" stroke="url(#neon-violet)" stroke-width="26" stroke-linecap="round" stroke-linejoin="round"/>

    <!-- Trailing dot / accent -->
    <circle cx="418" cy="342" r="8" fill="#38bdf8"/>
    """
    icons["02os-menu"] = (defs_02, "bg-02", art_02)

    # 2. thunar (Files / Finder)
    defs_thunar = """
    <linearGradient id="bg-thunar" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#38bdf8"/>
      <stop offset="50%" stop-color="#0284c7"/>
      <stop offset="100%" stop-color="#0369a1"/>
    </linearGradient>
    <linearGradient id="finder-left" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#93c5fd"/>
      <stop offset="100%" stop-color="#3b82f6"/>
    </linearGradient>
    <linearGradient id="finder-right" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#2563eb"/>
      <stop offset="100%" stop-color="#1d4ed8"/>
    </linearGradient>
    """
    art_thunar = """
    <!-- Finder Face Base Plate -->
    <rect x="96" y="96" width="320" height="320" rx="64" ry="64" fill="#0f172a" opacity="0.2"/>
    <g>
      <!-- Left Half Face -->
      <path d="M 112 160 C 112 133, 133 112, 160 112 L 256 112 L 256 400 L 160 400 C 133 400, 112 379, 112 352 Z" fill="url(#finder-left)"/>
      <!-- Right Half Face -->
      <path d="M 256 112 L 352 112 C 379 112, 400 133, 400 160 L 400 352 C 400 379, 379 400, 352 400 L 256 400 Z" fill="url(#finder-right)"/>
      
      <!-- Center Partition Line -->
      <line x1="256" y1="112" x2="256" y2="400" stroke="#1e3a8a" stroke-width="4"/>

      <!-- Eyes -->
      <ellipse cx="180" cy="220" rx="14" ry="24" fill="#0f172a"/>
      <ellipse cx="184" cy="214" rx="5" ry="8" fill="#ffffff"/>
      <ellipse cx="332" cy="220" rx="14" ry="24" fill="#0f172a"/>
      <ellipse cx="336" cy="214" rx="5" ry="8" fill="#ffffff"/>

      <!-- Friendly Smile Line -->
      <path d="M 180 300 Q 256 360 332 300" fill="none" stroke="#0f172a" stroke-width="16" stroke-linecap="round"/>
      <path d="M 180 300 Q 256 360 332 300" fill="none" stroke="#ffffff" stroke-width="6" stroke-linecap="round" opacity="0.3"/>
    </g>
    """
    icons["thunar"] = (defs_thunar, "bg-thunar", art_thunar)

    # 3. kitty (Terminal)
    defs_kitty = """
    <linearGradient id="bg-kitty" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#181920"/>
      <stop offset="100%" stop-color="#0b0c0e"/>
    </linearGradient>
    <linearGradient id="screen-grad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#0f1218"/>
      <stop offset="100%" stop-color="#07080a"/>
    </linearGradient>
    """
    art_kitty = """
    <!-- Terminal Screen Recess -->
    <rect x="80" y="80" width="352" height="352" rx="36" ry="36" fill="url(#screen-grad)" stroke="#27272a" stroke-width="2"/>

    <!-- Titlebar Jelly Dots -->
    <circle cx="116" cy="116" r="9" fill="#ff5f56"/>
    <circle cx="144" cy="116" r="9" fill="#ffbd2e"/>
    <circle cx="172" cy="116" r="9" fill="#27c93f"/>

    <!-- Kitty Silhouette Emblem -->
    <path d="M 330 110 L 348 132 L 372 110 L 378 146 C 378 160, 368 170, 351 170 C 334 170, 324 160, 324 146 Z" fill="#818cf8" opacity="0.85"/>
    <polygon points="334,116 344,130 332,132" fill="#f472b6"/>
    <polygon points="368,116 358,130 370,132" fill="#f472b6"/>

    <!-- Prompt Chevron '>' -->
    <path d="M 126 190 L 196 240 L 126 290" fill="none" stroke="#22d3ee" stroke-width="22" stroke-linecap="round" stroke-linejoin="round"/>

    <!-- Cursor '_' -->
    <line x1="226" y1="290" x2="310" y2="290" stroke="#4ade80" stroke-width="22" stroke-linecap="round"/>
    
    <!-- Code line preview -->
    <line x1="126" y1="350" x2="260" y2="350" stroke="#64748b" stroke-width="10" stroke-linecap="round" opacity="0.6"/>
    <line x1="280" y1="350" x2="380" y2="350" stroke="#38bdf8" stroke-width="10" stroke-linecap="round" opacity="0.5"/>
    """
    icons["kitty"] = (defs_kitty, "bg-kitty", art_kitty)

    # 4. firefox (Browser)
    defs_firefox = """
    <linearGradient id="bg-firefox" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#311042"/>
      <stop offset="40%" stop-color="#581c87"/>
      <stop offset="80%" stop-color="#9333ea"/>
      <stop offset="100%" stop-color="#c026d3"/>
    </linearGradient>
    <radialGradient id="globe-glow" cx="45%" cy="45%" r="55%">
      <stop offset="0%" stop-color="#38bdf8"/>
      <stop offset="50%" stop-color="#1d4ed8"/>
      <stop offset="100%" stop-color="#0f172a"/>
    </radialGradient>
    <linearGradient id="fox-flame" x1="0" y1="1" x2="1" y2="0">
      <stop offset="0%" stop-color="#ea580c"/>
      <stop offset="35%" stop-color="#f97316"/>
      <stop offset="70%" stop-color="#f59e0b"/>
      <stop offset="100%" stop-color="#fef08a"/>
    </linearGradient>
    """
    art_firefox = """
    <!-- Terrestrial Globe -->
    <circle cx="256" cy="256" r="128" fill="url(#globe-glow)"/>
    <ellipse cx="256" cy="256" rx="128" ry="46" fill="none" stroke="#60a5fa" stroke-width="3" opacity="0.4"/>
    <line x1="256" y1="128" x2="256" y2="384" stroke="#60a5fa" stroke-width="3" opacity="0.3"/>

    <!-- Circling Solar Flame Fox Tail & Head -->
    <path d="M 160 380 C 240 430, 360 400, 400 320 C 430 250, 410 170, 340 120 C 310 100, 270 90, 240 100 C 270 125, 285 160, 270 195 C 260 215, 235 225, 215 220 C 190 215, 175 190, 180 165 C 150 195, 130 240, 132 290 C 134 325, 142 355, 160 380 Z" fill="url(#fox-flame)"/>

    <!-- Fox Ear & Profile Highlight -->
    <polygon points="240,100 280,72 278,118" fill="#ea580c"/>
    <polygon points="246,102 274,82 272,114" fill="#fef08a"/>
    <circle cx="270" cy="148" r="8" fill="#ffffff"/>
    """
    icons["firefox"] = (defs_firefox, "bg-firefox", art_firefox)

    # 5. pavucontrol (Sound / Volume Control)
    defs_sound = """
    <linearGradient id="bg-sound" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#1e1b4b"/>
      <stop offset="50%" stop-color="#2e1065"/>
      <stop offset="100%" stop-color="#3b0764"/>
    </linearGradient>
    <linearGradient id="fader-knob" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f8fafc"/>
      <stop offset="100%" stop-color="#94a3b8"/>
    </linearGradient>
    """
    art_sound = """
    <!-- Studio Mixer Fader Tracks -->
    <!-- Track 1 -->
    <rect x="146" y="110" width="12" height="292" rx="6" fill="#0f172a" stroke="#334155" stroke-width="2"/>
    <!-- Track 2 -->
    <rect x="250" y="110" width="12" height="292" rx="6" fill="#0f172a" stroke="#334155" stroke-width="2"/>
    <!-- Track 3 -->
    <rect x="354" y="110" width="12" height="292" rx="6" fill="#0f172a" stroke="#334155" stroke-width="2"/>

    <!-- VU Meter Bars (Left track) -->
    <rect x="120" y="340" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="120" y="318" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="120" y="296" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="120" y="274" width="16" height="12" rx="2" fill="#f59e0b"/>
    <rect x="120" y="252" width="16" height="12" rx="2" fill="#ef4444"/>

    <!-- VU Meter Bars (Middle track) -->
    <rect x="224" y="340" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="224" y="318" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="224" y="296" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="224" y="274" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="224" y="252" width="16" height="12" rx="2" fill="#f59e0b"/>
    <rect x="224" y="230" width="16" height="12" rx="2" fill="#f59e0b"/>
    <rect x="224" y="208" width="16" height="12" rx="2" fill="#ef4444"/>
    <rect x="224" y="186" width="16" height="12" rx="2" fill="#ef4444"/>

    <!-- VU Meter Bars (Right track) -->
    <rect x="328" y="340" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="328" y="318" width="16" height="12" rx="2" fill="#10b981"/>
    <rect x="328" y="296" width="16" height="12" rx="2" fill="#f59e0b"/>

    <!-- Fader Knobs -->
    <!-- Knob 1 -->
    <g transform="translate(132, 240)">
      <rect x="0" y="0" width="40" height="26" rx="6" fill="url(#fader-knob)" stroke="#475569" stroke-width="2"/>
      <line x1="6" y1="13" x2="34" y2="13" stroke="#0284c7" stroke-width="3"/>
    </g>
    <!-- Knob 2 -->
    <g transform="translate(236, 170)">
      <rect x="0" y="0" width="40" height="26" rx="6" fill="url(#fader-knob)" stroke="#475569" stroke-width="2"/>
      <line x1="6" y1="13" x2="34" y2="13" stroke="#a855f7" stroke-width="3"/>
    </g>
    <!-- Knob 3 -->
    <g transform="translate(340, 280)">
      <rect x="0" y="0" width="40" height="26" rx="6" fill="url(#fader-knob)" stroke="#475569" stroke-width="2"/>
      <line x1="6" y1="13" x2="34" y2="13" stroke="#0284c7" stroke-width="3"/>
    </g>
    """
    icons["pavucontrol"] = (defs_sound, "bg-sound", art_sound)

    # 6. 02os-install (Installer)
    defs_install = """
    <linearGradient id="bg-install" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#042f2e"/>
      <stop offset="50%" stop-color="#0f766e"/>
      <stop offset="100%" stop-color="#0284c7"/>
    </linearGradient>
    <linearGradient id="rocket-body" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#ffffff"/>
      <stop offset="100%" stop-color="#94a3b8"/>
    </linearGradient>
    <linearGradient id="flame-grad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#fde047"/>
      <stop offset="50%" stop-color="#f97316"/>
      <stop offset="100%" stop-color="#ef4444"/>
    </linearGradient>
    """
    art_install = """
    <!-- Holographic Disc Base -->
    <ellipse cx="256" cy="350" rx="140" ry="46" fill="#0f172a" opacity="0.4"/>
    <ellipse cx="256" cy="344" rx="136" ry="42" fill="none" stroke="#22d3ee" stroke-width="8" opacity="0.6"/>
    <ellipse cx="256" cy="344" rx="80" ry="24" fill="none" stroke="#38bdf8" stroke-width="4" opacity="0.8"/>

    <!-- Thruster Flame -->
    <path d="M 236 290 Q 256 360 256 380 Q 256 360 276 290 Z" fill="url(#flame-grad)"/>

    <!-- Ascending Rocket -->
    <g transform="translate(0, -20)">
      <!-- Fins -->
      <polygon points="200,290 230,240 230,290" fill="#0284c7"/>
      <polygon points="312,290 282,240 282,290" fill="#0284c7"/>
      <polygon points="256,295 244,240 268,240" fill="#0369a1"/>

      <!-- Rocket Body -->
      <path d="M 256 120 C 230 180, 226 250, 230 290 L 282 290 C 286 250, 282 180, 256 120 Z" fill="url(#rocket-body)"/>

      <!-- Window / Porthole -->
      <circle cx="256" cy="190" r="18" fill="#0284c7" stroke="#38bdf8" stroke-width="4"/>
      <circle cx="252" cy="186" r="5" fill="#ffffff"/>

      <!-- 02 Accent line -->
      <path d="M 248 240 L 264 240" stroke="#0f172a" stroke-width="4" stroke-linecap="round"/>
    </g>
    """
    icons["02os-install"] = (defs_install, "bg-install", art_install)

    # 7. 02os-spotlight (Fuzzel / Spotlight Search)
    defs_spotlight = """
    <linearGradient id="bg-spotlight" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#1f242e"/>
      <stop offset="100%" stop-color="#0a0c10"/>
    </linearGradient>
    <linearGradient id="prism-lens" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#38bdf8" stop-opacity="0.6"/>
      <stop offset="50%" stop-color="#a855f7" stop-opacity="0.4"/>
      <stop offset="100%" stop-color="#f43f5e" stop-opacity="0.6"/>
    </linearGradient>
    <linearGradient id="handle-grad" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#e2e8f0"/>
      <stop offset="50%" stop-color="#94a3b8"/>
      <stop offset="100%" stop-color="#475569"/>
    </linearGradient>
    """
    art_spotlight = """
    <!-- Prism Lens Shadow -->
    <circle cx="216" cy="216" r="94" fill="#000000" opacity="0.3"/>

    <!-- Lens Ring -->
    <circle cx="216" cy="216" r="92" fill="none" stroke="#f1f5f9" stroke-width="20"/>
    <circle cx="216" cy="216" r="82" fill="url(#prism-lens)"/>
    <circle cx="216" cy="216" r="82" fill="none" stroke="#ffffff" stroke-width="4" opacity="0.6"/>

    <!-- Lens Specular Arc -->
    <path d="M 150 180 C 160 150, 190 140, 220 140" fill="none" stroke="#ffffff" stroke-width="10" stroke-linecap="round" opacity="0.8"/>

    <!-- Handle -->
    <line x1="284" y1="284" x2="384" y2="384" stroke="url(#handle-grad)" stroke-width="28" stroke-linecap="round"/>
    <line x1="324" y1="324" x2="384" y2="384" stroke="#0f172a" stroke-width="18" stroke-linecap="round" opacity="0.3"/>

    <!-- Star Glints -->
    <polygon points="340,140 344,152 356,156 344,160 340,172 336,160 324,156 336,152" fill="#ffffff"/>
    <polygon points="130,290 132,298 140,300 132,302 130,310 128,302 120,300 128,298" fill="#ffffff" opacity="0.8"/>
    """
    icons["02os-spotlight"] = (defs_spotlight, "bg-spotlight", art_spotlight)

    # 8. swaync (Notifications)
    defs_swaync = """
    <linearGradient id="bg-swaync" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#881337"/>
      <stop offset="50%" stop-color="#e11d48"/>
      <stop offset="100%" stop-color="#fb7185"/>
    </linearGradient>
    <linearGradient id="bell-grad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff"/>
      <stop offset="60%" stop-color="#fef08a"/>
      <stop offset="100%" stop-color="#f59e0b"/>
    </linearGradient>
    """
    art_swaync = """
    <!-- Chime Waves -->
    <path d="M 120 220 C 100 240, 100 272, 120 292" fill="none" stroke="#ffffff" stroke-width="8" stroke-linecap="round" opacity="0.6"/>
    <path d="M 392 220 C 412 240, 412 272, 392 292" fill="none" stroke="#ffffff" stroke-width="8" stroke-linecap="round" opacity="0.6"/>

    <!-- Bell Top Loop -->
    <ellipse cx="256" cy="140" rx="20" ry="14" fill="none" stroke="url(#bell-grad)" stroke-width="12"/>

    <!-- Bell Body -->
    <path d="M 256 150 C 210 150, 176 190, 176 256 C 176 300, 150 326, 140 336 L 372 336 C 362 326, 336 300, 336 256 C 336 190, 302 150, 256 150 Z" fill="url(#bell-grad)"/>

    <!-- Bell Clapper -->
    <ellipse cx="256" cy="356" rx="28" ry="18" fill="#d97706"/>

    <!-- Glowing Alert Badge -->
    <circle cx="360" cy="140" r="28" fill="#ffffff" filter="url(#glow)"/>
    <circle cx="360" cy="140" r="24" fill="#0284c7"/>
    <circle cx="360" cy="140" r="10" fill="#ffffff"/>
    """
    icons["swaync"] = (defs_swaync, "bg-swaync", art_swaync)

    # 9. blueman (Bluetooth)
    defs_blueman = """
    <linearGradient id="bg-blueman" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#1d4ed8"/>
      <stop offset="50%" stop-color="#2563eb"/>
      <stop offset="100%" stop-color="#3b82f6"/>
    </linearGradient>
    """
    art_blueman = """
    <!-- Nordic Rune Bluetooth Symbol -->
    <path d="M 256 110 L 256 402 M 256 110 L 336 182 L 206 312 M 206 200 L 336 330 L 256 402" 
          fill="none" stroke="#ffffff" stroke-width="26" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M 256 110 L 256 402 M 256 110 L 336 182 L 206 312 M 206 200 L 336 330 L 256 402" 
          fill="none" stroke="#93c5fd" stroke-width="12" stroke-linecap="round" stroke-linejoin="round" opacity="0.6"/>

    <!-- Subtle Wireless Pulse Dots -->
    <circle cx="150" cy="256" r="8" fill="#ffffff" opacity="0.6"/>
    <circle cx="362" cy="256" r="8" fill="#ffffff" opacity="0.6"/>
    """
    icons["blueman"] = (defs_blueman, "bg-blueman", art_blueman)

    # 10. nm-connection-editor (Network / Wi-Fi)
    defs_wifi = """
    <linearGradient id="bg-wifi" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#064e3b"/>
      <stop offset="50%" stop-color="#059669"/>
      <stop offset="100%" stop-color="#10b981"/>
    </linearGradient>
    """
    art_wifi = """
    <!-- 4-Tier Wi-Fi Waves -->
    <!-- Arc 1 -->
    <path d="M 120 180 C 196 110, 316 110, 392 180" fill="none" stroke="#ffffff" stroke-width="24" stroke-linecap="round"/>
    <!-- Arc 2 -->
    <path d="M 160 232 C 214 182, 298 182, 352 232" fill="none" stroke="#ffffff" stroke-width="24" stroke-linecap="round"/>
    <!-- Arc 3 -->
    <path d="M 200 284 C 232 254, 280 254, 312 284" fill="none" stroke="#ffffff" stroke-width="24" stroke-linecap="round"/>
    <!-- Center Dot -->
    <circle cx="256" cy="346" r="22" fill="#ffffff"/>
    """
    icons["nm-connection-editor"] = (defs_wifi, "bg-wifi", art_wifi)

    # 11. 02os-screenshot (Screenshot / Screen Capture)
    defs_camera = """
    <linearGradient id="bg-camera" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#1e293b"/>
      <stop offset="50%" stop-color="#334155"/>
      <stop offset="100%" stop-color="#475569"/>
    </linearGradient>
    <linearGradient id="lens-iris" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#6366f1"/>
      <stop offset="50%" stop-color="#3b82f6"/>
      <stop offset="100%" stop-color="#06b6d4"/>
    </linearGradient>
    """
    art_camera = """
    <!-- Viewfinder Brackets -->
    <path d="M 110 160 L 110 110 L 160 110" fill="none" stroke="#38bdf8" stroke-width="12" stroke-linecap="round"/>
    <path d="M 402 160 L 402 110 L 352 110" fill="none" stroke="#38bdf8" stroke-width="12" stroke-linecap="round"/>
    <path d="M 110 352 L 110 402 L 160 402" fill="none" stroke="#38bdf8" stroke-width="12" stroke-linecap="round"/>
    <path d="M 402 352 L 402 402 L 352 402" fill="none" stroke="#38bdf8" stroke-width="12" stroke-linecap="round"/>

    <!-- Lens Rim Outer -->
    <circle cx="256" cy="256" r="110" fill="#0f172a" stroke="#64748b" stroke-width="8"/>
    <!-- Lens Coated Glass -->
    <circle cx="256" cy="256" r="88" fill="url(#lens-iris)"/>
    <!-- Aperture Opening -->
    <circle cx="256" cy="256" r="50" fill="#090d16"/>
    <!-- Specular Highlight -->
    <ellipse cx="230" cy="226" rx="24" ry="14" fill="#ffffff" opacity="0.6"/>
    """
    icons["02os-screenshot"] = (defs_camera, "bg-camera", art_camera)

    # 12. fastfetch (System Telemetry)
    defs_fetch = """
    <linearGradient id="bg-fetch" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#030712"/>
      <stop offset="50%" stop-color="#111827"/>
      <stop offset="100%" stop-color="#1f2937"/>
    </linearGradient>
    <linearGradient id="chip-grad" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#1e293b"/>
      <stop offset="100%" stop-color="#0f172a"/>
    </linearGradient>
    """
    art_fetch = """
    <!-- CPU Chip Die -->
    <rect x="136" y="136" width="240" height="240" rx="24" ry="24" fill="url(#chip-grad)" stroke="#38bdf8" stroke-width="6"/>

    <!-- Gold Contact Pins -->
    <!-- Top & Bottom Pins -->
    <g stroke="#f59e0b" stroke-width="6" stroke-linecap="round">
      <line x1="176" y1="116" x2="176" y2="136"/><line x1="216" y1="116" x2="216" y2="136"/>
      <line x1="256" y1="116" x2="256" y2="136"/><line x1="296" y1="116" x2="296" y2="136"/>
      <line x1="336" y1="116" x2="336" y2="136"/>
      <line x1="176" y1="376" x2="176" y2="396"/><line x1="216" y1="376" x2="216" y2="396"/>
      <line x1="256" y1="376" x2="256" y2="396"/><line x1="296" y1="376" x2="296" y2="396"/>
      <line x1="336" y1="376" x2="336" y2="396"/>
    </g>

    <!-- Pulse Wave ECG -->
    <path d="M 156 256 L 206 256 L 226 210 L 256 310 L 286 230 L 306 256 L 356 256" 
          fill="none" stroke="#22d3ee" stroke-width="12" stroke-linecap="round" stroke-linejoin="round"/>
    """
    icons["fastfetch"] = (defs_fetch, "bg-fetch", art_fetch)

    # 13. gnome-control-center (Settings)
    defs_settings = """
    <linearGradient id="bg-settings" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#334155"/>
      <stop offset="50%" stop-color="#475569"/>
      <stop offset="100%" stop-color="#64748b"/>
    </linearGradient>
    <linearGradient id="gear-metal" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#f8fafc"/>
      <stop offset="100%" stop-color="#94a3b8"/>
    </linearGradient>
    """
    art_settings = """
    <!-- Gear Assembly -->
    <g transform="translate(230, 260)">
      <!-- Main Gear -->
      <circle cx="0" cy="0" r="100" fill="none" stroke="url(#gear-metal)" stroke-width="36" stroke-dasharray="32, 20.3"/>
      <circle cx="0" cy="0" r="54" fill="#1e293b"/>
      <circle cx="0" cy="0" r="24" fill="#38bdf8"/>
    </g>
    <g transform="translate(330, 160)">
      <!-- Smaller Gear -->
      <circle cx="0" cy="0" r="60" fill="none" stroke="url(#gear-metal)" stroke-width="24" stroke-dasharray="20, 11.4"/>
      <circle cx="0" cy="0" r="32" fill="#1e293b"/>
      <circle cx="0" cy="0" r="14" fill="#a855f7"/>
    </g>
    """
    icons["gnome-control-center"] = (defs_settings, "bg-settings", art_settings)

    # 14. gnome-tweaks (Tweaks)
    defs_tweaks = """
    <linearGradient id="bg-tweaks" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#581c87"/>
      <stop offset="50%" stop-color="#7e22ce"/>
      <stop offset="100%" stop-color="#9333ea"/>
    </linearGradient>
    """
    art_tweaks = """
    <!-- Slider Track 1 -->
    <line x1="120" y1="180" x2="392" y2="180" stroke="#3b0764" stroke-width="16" stroke-linecap="round"/>
    <line x1="120" y1="180" x2="280" y2="180" stroke="#38bdf8" stroke-width="16" stroke-linecap="round"/>
    <circle cx="280" cy="180" r="24" fill="#ffffff"/>

    <!-- Slider Track 2 -->
    <line x1="120" y1="256" x2="392" y2="256" stroke="#3b0764" stroke-width="16" stroke-linecap="round"/>
    <line x1="120" y1="256" x2="190" y2="256" stroke="#c084fc" stroke-width="16" stroke-linecap="round"/>
    <circle cx="190" cy="256" r="24" fill="#ffffff"/>

    <!-- Slider Track 3 -->
    <line x1="120" y1="332" x2="392" y2="332" stroke="#3b0764" stroke-width="16" stroke-linecap="round"/>
    <line x1="120" y1="332" x2="340" y2="332" stroke="#f472b6" stroke-width="16" stroke-linecap="round"/>
    <circle cx="340" cy="332" r="24" fill="#ffffff"/>
    """
    icons["gnome-tweaks"] = (defs_tweaks, "bg-tweaks", art_tweaks)

    # 15. xarchiver (Archive Manager)
    defs_archive = """
    <linearGradient id="bg-archive" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#78350f"/>
      <stop offset="50%" stop-color="#b45309"/>
      <stop offset="100%" stop-color="#f59e0b"/>
    </linearGradient>
    """
    art_archive = """
    <!-- Parcel Chest -->
    <rect x="110" y="110" width="292" height="292" rx="36" ry="36" fill="#451a03" stroke="#fef3c7" stroke-width="4"/>
    
    <!-- Zipper Vertical Strip -->
    <rect x="236" y="110" width="40" height="292" fill="#1e293b"/>
    <!-- Interlocking Teeth -->
    <g fill="#94a3b8">
      <rect x="238" y="130" width="16" height="10"/>
      <rect x="258" y="145" width="16" height="10"/>
      <rect x="238" y="160" width="16" height="10"/>
      <rect x="258" y="175" width="16" height="10"/>
      <rect x="238" y="190" width="16" height="10"/>
      <rect x="258" y="205" width="16" height="10"/>
      <rect x="238" y="220" width="16" height="10"/>
      <rect x="258" y="235" width="16" height="10"/>
      <rect x="238" y="250" width="16" height="10"/>
      <rect x="258" y="265" width="16" height="10"/>
      <rect x="238" y="280" width="16" height="10"/>
      <rect x="258" y="295" width="16" height="10"/>
      <rect x="238" y="310" width="16" height="10"/>
      <rect x="258" y="325" width="16" height="10"/>
      <rect x="238" y="340" width="16" height="10"/>
      <rect x="258" y="355" width="16" height="10"/>
    </g>

    <!-- Zipper Pull Slider -->
    <g transform="translate(230, 220)">
      <rect x="0" y="0" width="52" height="42" rx="8" fill="#f8fafc" stroke="#64748b" stroke-width="2"/>
      <ellipse cx="26" cy="62" rx="14" ry="24" fill="#cbd5e1" stroke="#475569" stroke-width="3"/>
    </g>
    """
    icons["xarchiver"] = (defs_archive, "bg-archive", art_archive)

    # 16. accessories-text-editor (Text & Code Editor)
    defs_editor = """
    <linearGradient id="bg-editor" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#064e3b"/>
      <stop offset="50%" stop-color="#047857"/>
      <stop offset="100%" stop-color="#10b981"/>
    </linearGradient>
    """
    art_editor = """
    <!-- Paper Sheet -->
    <path d="M 130 100 L 320 100 L 382 162 L 382 412 L 130 412 Z" fill="#ffffff"/>
    <path d="M 320 100 L 320 162 L 382 162 Z" fill="#cbd5e1"/>

    <!-- Code Syntax Lines -->
    <line x1="170" y1="160" x2="260" y2="160" stroke="#6366f1" stroke-width="12" stroke-linecap="round"/>
    <line x1="170" y1="204" x2="340" y2="204" stroke="#0284c7" stroke-width="12" stroke-linecap="round"/>
    <line x1="200" y1="248" x2="310" y2="248" stroke="#10b981" stroke-width="12" stroke-linecap="round"/>
    <line x1="200" y1="292" x2="280" y2="292" stroke="#f59e0b" stroke-width="12" stroke-linecap="round"/>
    <line x1="170" y1="336" x2="230" y2="336" stroke="#ef4444" stroke-width="12" stroke-linecap="round"/>

    <!-- Drafting Pen Nib -->
    <g transform="translate(320, 310) rotate(-45)">
      <path d="M 0 0 L 24 70 L 12 90 L 0 70 Z" fill="#f59e0b"/>
      <line x1="12" y1="40" x2="12" y2="90" stroke="#0f172a" stroke-width="2"/>
    </g>
    """
    icons["accessories-text-editor"] = (defs_editor, "bg-editor", art_editor)

    # 17. accessories-calculator (Calculator)
    defs_calc = """
    <linearGradient id="bg-calc" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#18181b"/>
      <stop offset="100%" stop-color="#27272a"/>
    </linearGradient>
    """
    art_calc = """
    <!-- LCD Display -->
    <rect x="100" y="100" width="312" height="74" rx="18" ry="18" fill="#09090b" stroke="#3f3f46" stroke-width="2"/>
    <!-- LED Output -->
    <text x="386" y="152" fill="#22c55e" font-family="monospace" font-size="44" font-weight="bold" text-anchor="end">02_OS</text>

    <!-- Keypad Grid -->
    <!-- Numbers -->
    <circle cx="140" cy="226" r="24" fill="#3f3f46"/>
    <circle cx="216" cy="226" r="24" fill="#3f3f46"/>
    <circle cx="292" cy="226" r="24" fill="#3f3f46"/>
    <circle cx="140" cy="296" r="24" fill="#3f3f46"/>
    <circle cx="216" cy="296" r="24" fill="#3f3f46"/>
    <circle cx="292" cy="296" r="24" fill="#3f3f46"/>
    <circle cx="140" cy="366" r="24" fill="#3f3f46"/>
    <circle cx="216" cy="366" r="24" fill="#3f3f46"/>
    <circle cx="292" cy="366" r="24" fill="#3f3f46"/>

    <!-- Operators (Orange column) -->
    <circle cx="368" cy="226" r="24" fill="#f97316"/>
    <circle cx="368" cy="296" r="24" fill="#f97316"/>
    <circle cx="368" cy="366" r="24" fill="#ea580c"/>
    """
    icons["accessories-calculator"] = (defs_calc, "bg-calc", art_calc)

    # 18. document-viewer (PDF / Reader)
    defs_doc = """
    <linearGradient id="bg-doc" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#881337"/>
      <stop offset="50%" stop-color="#dc2626"/>
      <stop offset="100%" stop-color="#ef4444"/>
    </linearGradient>
    """
    art_doc = """
    <!-- Open Book / Folio -->
    <path d="M 120 120 L 392 120 C 392 240, 360 380, 256 380 C 152 380, 120 240, 120 120 Z" fill="#ffffff"/>
    
    <!-- Red Ribbon Bookmark -->
    <polygon points="236,100 276,100 276,260 256,240 236,260" fill="#dc2626"/>

    <!-- Typography Lines -->
    <line x1="160" y1="180" x2="220" y2="180" stroke="#94a3b8" stroke-width="8" stroke-linecap="round"/>
    <line x1="160" y1="210" x2="220" y2="210" stroke="#94a3b8" stroke-width="8" stroke-linecap="round"/>
    <line x1="290" y1="180" x2="350" y2="180" stroke="#94a3b8" stroke-width="8" stroke-linecap="round"/>
    <line x1="290" y1="210" x2="350" y2="210" stroke="#94a3b8" stroke-width="8" stroke-linecap="round"/>
    """
    icons["document-viewer"] = (defs_doc, "bg-doc", art_doc)

    # 19. multimedia-photo-viewer (Photos)
    defs_photo = """
    <linearGradient id="bg-photo" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#3b0764"/>
      <stop offset="50%" stop-color="#701a75"/>
      <stop offset="100%" stop-color="#db2777"/>
    </linearGradient>
    """
    art_photo = """
    <!-- Polaroid Frame -->
    <rect x="96" y="96" width="320" height="320" rx="28" ry="28" fill="#ffffff"/>
    <rect x="116" y="116" width="280" height="230" rx="16" ry="16" fill="#0f172a"/>

    <!-- Sun -->
    <circle cx="320" cy="180" r="26" fill="#fbbf24"/>

    <!-- Mountains -->
    <polygon points="116,346 220,200 290,300 350,230 396,346" fill="#0284c7"/>
    <polygon points="116,346 220,200 250,250 180,346" fill="#38bdf8"/>
    """
    icons["multimedia-photo-viewer"] = (defs_photo, "bg-photo", art_photo)

    # 20. multimedia-player (Media Player)
    defs_media = """
    <linearGradient id="bg-media" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#312e81"/>
      <stop offset="50%" stop-color="#4f46e5"/>
      <stop offset="100%" stop-color="#7c3aed"/>
    </linearGradient>
    """
    art_media = """
    <!-- Harmonic Pulse Rings -->
    <circle cx="256" cy="256" r="140" fill="none" stroke="#818cf8" stroke-width="4" opacity="0.3"/>
    <circle cx="256" cy="256" r="110" fill="none" stroke="#818cf8" stroke-width="6" opacity="0.5"/>

    <!-- Translucent Play Triangle -->
    <polygon points="216,180 340,256 216,332" fill="#ffffff"/>
    """
    icons["multimedia-player"] = (defs_media, "bg-media", art_media)

    # 21. system-lock-screen (Lock)
    defs_lock = """
    <linearGradient id="bg-lock" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#0f172a"/>
      <stop offset="100%" stop-color="#1e293b"/>
    </linearGradient>
    <linearGradient id="shackle-grad" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#fde047"/>
      <stop offset="100%" stop-color="#ca8a04"/>
    </linearGradient>
    """
    art_lock = """
    <!-- Padlock Shackle -->
    <path d="M 196 230 L 196 170 C 196 137, 223 110, 256 110 C 289 110, 316 137, 316 170 L 316 230" 
          fill="none" stroke="url(#shackle-grad)" stroke-width="32" stroke-linecap="round"/>

    <!-- Padlock Body -->
    <rect x="156" y="220" width="200" height="170" rx="32" ry="32" fill="url(#shackle-grad)"/>

    <!-- Illuminated Keyhole -->
    <circle cx="256" cy="290" r="18" fill="#0f172a"/>
    <polygon points="248,290 264,290 268,340 244,340" fill="#0f172a"/>
    <circle cx="256" cy="290" r="6" fill="#38bdf8"/>
    """
    icons["system-lock-screen"] = (defs_lock, "bg-lock", art_lock)

    # 22. system-shutdown
    defs_shutdown = """
    <linearGradient id="bg-shutdown" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#7f1d1d"/>
      <stop offset="100%" stop-color="#dc2626"/>
    </linearGradient>
    """
    art_shutdown = """
    <!-- Power Icon -->
    <path d="M 196 170 C 156 200, 150 260, 180 306 C 210 352, 280 364, 326 330 C 366 300, 370 240, 340 196 C 332 184, 320 174, 316 170" 
          fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round"/>
    <line x1="256" y1="110" x2="256" y2="230" stroke="#ffffff" stroke-width="28" stroke-linecap="round"/>
    """
    icons["system-shutdown"] = (defs_shutdown, "bg-shutdown", art_shutdown)

    # 23. system-reboot
    defs_reboot = """
    <linearGradient id="bg-reboot" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#78350f"/>
      <stop offset="100%" stop-color="#d97706"/>
    </linearGradient>
    """
    art_reboot = """
    <!-- Restart Circular Arrows -->
    <path d="M 140 256 C 140 180, 200 130, 280 130 C 330 130, 368 156, 386 190" fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round"/>
    <polygon points="386,140 406,198 348,198" fill="#ffffff"/>

    <path d="M 372 256 C 372 332, 312 382, 232 382 C 182 382, 144 356, 126 322" fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round"/>
    <polygon points="126,372 106,314 164,314" fill="#ffffff"/>
    """
    icons["system-reboot"] = (defs_reboot, "bg-reboot", art_reboot)

    # 24. system-suspend
    defs_suspend = """
    <linearGradient id="bg-suspend" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#172554"/>
      <stop offset="100%" stop-color="#1e3a8a"/>
    </linearGradient>
    """
    art_suspend = """
    <!-- Crescent Moon & Stars -->
    <path d="M 280 130 C 200 140, 150 210, 160 290 C 170 360, 230 400, 300 390 C 240 360, 220 280, 260 210 C 280 170, 320 140, 360 140 C 330 130, 305 130, 280 130 Z" fill="#fde047"/>
    <polygon points="350,220 354,228 362,230 354,232 350,240 346,232 338,230 346,228" fill="#ffffff"/>
    <polygon points="320,310 322,316 328,318 322,320 320,326 318,320 312,318 318,316" fill="#ffffff" opacity="0.8"/>
    """
    icons["system-suspend"] = (defs_suspend, "bg-suspend", art_suspend)

    # 25. system-log-out
    defs_logout = """
    <linearGradient id="bg-logout" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#334155"/>
      <stop offset="100%" stop-color="#475569"/>
    </linearGradient>
    """
    art_logout = """
    <!-- Open Door -->
    <rect x="140" y="110" width="130" height="292" rx="12" fill="#0f172a" stroke="#ffffff" stroke-width="12"/>
    <!-- Exit Arrow -->
    <path d="M 230 256 L 372 256 M 326 210 L 372 256 L 326 302" fill="none" stroke="#38bdf8" stroke-width="24" stroke-linecap="round" stroke-linejoin="round"/>
    """
    icons["system-log-out"] = (defs_logout, "bg-logout", art_logout)

    # 26. system-help
    defs_help = """
    <linearGradient id="bg-help" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#0369a1"/>
      <stop offset="100%" stop-color="#0284c7"/>
    </linearGradient>
    """
    art_help = """
    <!-- Lifebuoy Ring -->
    <circle cx="256" cy="256" r="120" fill="none" stroke="#ffffff" stroke-width="48"/>
    <!-- Red stripes -->
    <path d="M 256 136 L 256 184 M 256 328 L 256 376 M 136 256 L 184 256 M 328 256 L 376 256" stroke="#ef4444" stroke-width="48"/>
    <!-- Question Mark -->
    <text x="256" y="286" fill="#0284c7" font-family="Inter, sans-serif" font-size="96" font-weight="900" text-anchor="middle">?</text>
    """
    icons["system-help"] = (defs_help, "bg-help", art_help)

    # 27. git
    defs_git = """
    <linearGradient id="bg-git" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#9a3412"/>
      <stop offset="50%" stop-color="#ea580c"/>
      <stop offset="100%" stop-color="#f97316"/>
    </linearGradient>
    """
    art_git = """
    <!-- Git Branch Nodes -->
    <g transform="translate(256, 256) rotate(45) translate(-256, -256)">
      <rect x="146" y="146" width="220" height="220" rx="36" fill="#ffffff"/>
      <circle cx="216" cy="216" r="22" fill="#ea580c"/>
      <circle cx="216" cy="296" r="22" fill="#ea580c"/>
      <circle cx="296" cy="216" r="22" fill="#ea580c"/>
      <line x1="216" y1="216" x2="216" y2="296" stroke="#ea580c" stroke-width="14"/>
      <path d="M 216 296 C 216 256, 296 256, 296 216" fill="none" stroke="#ea580c" stroke-width="14"/>
    </g>
    """
    icons["git"] = (defs_git, "bg-git", art_git)

    # 28. software-properties (Updates)
    defs_software = """
    <linearGradient id="bg-soft" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#075985"/>
      <stop offset="100%" stop-color="#0284c7"/>
    </linearGradient>
    """
    art_software = """
    <!-- Box Package with Sync Arrows -->
    <polygon points="256,120 376,180 256,240 136,180" fill="#f8fafc"/>
    <polygon points="136,180 256,240 256,380 136,320" fill="#cbd5e1"/>
    <polygon points="376,180 256,240 256,380 376,320" fill="#94a3b8"/>
    <circle cx="360" cy="150" r="32" fill="#10b981"/>
    <path d="M 346 150 L 356 160 L 376 140" fill="none" stroke="#ffffff" stroke-width="6" stroke-linecap="round"/>
    """
    icons["software-properties"] = (defs_software, "bg-soft", art_software)

    return icons

print("icons_apps module ready.")
