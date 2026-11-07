# Status and Actions SVG definitions for 02-OS

def get_status_icons():
    icons = {}

    def status_circle(color, glyph):
        defs = f"""
        <linearGradient id="bg-stat" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="{color}"/>
          <stop offset="100%" stop-color="#0f172a"/>
        </linearGradient>
        """
        body = f"""
        <circle cx="256" cy="256" r="210" fill="url(#bg-stat)" filter="url(#shadow)"/>
        <circle cx="256" cy="256" r="210" fill="none" stroke="url(#specular-rim)" stroke-width="4"/>
        <g filter="url(#glyph-shadow)">
          {glyph}
        </g>
        """
        return defs, body

    # 1. dialog-information
    g_info = """
    <circle cx="256" cy="180" r="24" fill="#ffffff"/>
    <rect x="236" y="240" width="40" height="150" rx="16" fill="#ffffff"/>
    """
    d, b = status_circle("#0284c7", g_info)
    icons["dialog-information"] = (d, b)

    # 2. dialog-warning
    g_warn = """
    <polygon points="256,100 412,380 100,380" fill="#f59e0b"/>
    <rect x="244" y="200" width="24" height="100" rx="12" fill="#0f172a"/>
    <circle cx="256" cy="336" r="16" fill="#0f172a"/>
    """
    d, b = status_circle("#b45309", g_warn)
    icons["dialog-warning"] = (d, b)

    # 3. dialog-error
    g_err = """
    <line x1="170" y1="170" x2="342" y2="342" stroke="#ffffff" stroke-width="36" stroke-linecap="round"/>
    <line x1="342" y1="170" x2="170" y2="342" stroke="#ffffff" stroke-width="36" stroke-linecap="round"/>
    """
    d, b = status_circle("#dc2626", g_err)
    icons["dialog-error"] = (d, b)

    # 4. dialog-question
    g_quest = """
    <text x="256" y="340" fill="#ffffff" font-family="Inter, sans-serif" font-size="240" font-weight="bold" text-anchor="middle">?</text>
    """
    d, b = status_circle("#7c3aed", g_quest)
    icons["dialog-question"] = (d, b)

    # 5. dialog-password
    g_pass = """
    <circle cx="216" cy="216" r="60" fill="none" stroke="#fde047" stroke-width="24"/>
    <line x1="260" y1="260" x2="360" y2="360" stroke="#fde047" stroke-width="24" stroke-linecap="round"/>
    <line x1="330" y1="330" x2="360" y2="300" stroke="#fde047" stroke-width="20" stroke-linecap="round"/>
    """
    d, b = status_circle("#0f172a", g_pass)
    icons["dialog-password"] = (d, b)

    # 6. audio-volume-high
    g_vol_high = """
    <polygon points="120,200 190,200 270,130 270,382 190,312 120,312" fill="#ffffff"/>
    <path d="M 310 200 C 335 220, 335 292, 310 312" fill="none" stroke="#ffffff" stroke-width="16" stroke-linecap="round"/>
    <path d="M 346 160 C 390 195, 390 317, 346 352" fill="none" stroke="#ffffff" stroke-width="16" stroke-linecap="round"/>
    """
    d, b = status_circle("#2563eb", g_vol_high)
    icons["audio-volume-high"] = (d, b)

    # 7. audio-volume-medium
    g_vol_med = """
    <polygon points="120,200 190,200 270,130 270,382 190,312 120,312" fill="#ffffff"/>
    <path d="M 310 200 C 335 220, 335 292, 310 312" fill="none" stroke="#ffffff" stroke-width="16" stroke-linecap="round"/>
    """
    d, b = status_circle("#2563eb", g_vol_med)
    icons["audio-volume-medium"] = (d, b)

    # 8. audio-volume-low
    g_vol_low = """
    <polygon points="120,200 190,200 270,130 270,382 190,312 120,312" fill="#ffffff"/>
    """
    d, b = status_circle("#2563eb", g_vol_low)
    icons["audio-volume-low"] = (d, b)

    # 9. audio-volume-muted
    g_vol_mute = """
    <polygon points="120,200 190,200 270,130 270,382 190,312 120,312" fill="#64748b"/>
    <line x1="310" y1="210" x2="380" y2="280" stroke="#ef4444" stroke-width="18" stroke-linecap="round"/>
    <line x1="380" y1="210" x2="310" y2="280" stroke="#ef4444" stroke-width="18" stroke-linecap="round"/>
    """
    d, b = status_circle("#1e293b", g_vol_mute)
    icons["audio-volume-muted"] = (d, b)

    # 10. battery & battery-charging
    g_bat = """
    <rect x="110" y="196" width="260" height="120" rx="28" fill="none" stroke="#ffffff" stroke-width="16"/>
    <rect x="382" y="234" width="22" height="44" rx="8" fill="#ffffff"/>
    <rect x="126" y="212" width="200" height="88" rx="18" fill="#10b981"/>
    """
    d, b = status_circle("#047857", g_bat)
    icons["battery"] = (d, b)
    icons["battery-full"] = (d, b)

    g_bat_chg = g_bat + """
    <polygon points="256,160 216,256 256,256 240,330 286,236 246,236" fill="#facc15" stroke="#000000" stroke-width="6"/>
    """
    d, b = status_circle("#047857", g_bat_chg)
    icons["battery-charging"] = (d, b)

    # 11. battery-low
    g_bat_low = """
    <rect x="110" y="196" width="260" height="120" rx="28" fill="none" stroke="#ffffff" stroke-width="16"/>
    <rect x="382" y="234" width="22" height="44" rx="8" fill="#ffffff"/>
    <rect x="126" y="212" width="50" height="88" rx="14" fill="#ef4444"/>
    """
    d, b = status_circle("#7f1d1d", g_bat_low)
    icons["battery-low"] = (d, b)

    return icons

def get_actions_icons():
    icons = {}

    def action_btn(glyph):
        defs = """
        <linearGradient id="bg-act" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#334155"/>
          <stop offset="100%" stop-color="#1e293b"/>
        </linearGradient>
        """
        body = f"""
        <circle cx="256" cy="256" r="210" fill="url(#bg-act)" filter="url(#shadow)"/>
        <circle cx="256" cy="256" r="210" fill="none" stroke="url(#specular-rim)" stroke-width="4"/>
        <g filter="url(#glyph-shadow)">
          {glyph}
        </g>
        """
        return defs, body

    # 1. go-home
    g_home = """
    <path d="M 200 350 L 200 270 L 256 210 L 312 270 L 312 350 Z" fill="#ffffff"/>
    <path d="M 180 276 L 256 200 L 332 276" fill="none" stroke="#ffffff" stroke-width="14" stroke-linecap="round"/>
    """
    d, b = action_btn(g_home)
    icons["go-home"] = (d, b)

    # 2. go-next
    g_next = """
    <path d="M 210 156 L 310 256 L 210 356" fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/>
    """
    d, b = action_btn(g_next)
    icons["go-next"] = (d, b)

    # 3. go-previous
    g_prev = """
    <path d="M 302 156 L 202 256 L 302 356" fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/>
    """
    d, b = action_btn(g_prev)
    icons["go-previous"] = (d, b)

    # 4. go-up
    g_up = """
    <path d="M 156 302 L 256 202 L 356 302" fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/>
    """
    d, b = action_btn(g_up)
    icons["go-up"] = (d, b)

    # 5. go-down
    g_down = """
    <path d="M 156 210 L 256 310 L 356 210" fill="none" stroke="#ffffff" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/>
    """
    d, b = action_btn(g_down)
    icons["go-down"] = (d, b)

    # 6. view-refresh
    g_ref = """
    <path d="M 160 256 C 160 200, 204 156, 256 156 C 308 156, 348 190, 352 230" fill="none" stroke="#ffffff" stroke-width="24" stroke-linecap="round"/>
    <polygon points="352,180 376,234 322,234" fill="#ffffff"/>
    <path d="M 352 256 C 352 312, 308 356, 256 356 C 204 356, 164 322, 160 282" fill="none" stroke="#ffffff" stroke-width="24" stroke-linecap="round"/>
    <polygon points="160,332 136,278 190,278" fill="#ffffff"/>
    """
    d, b = action_btn(g_ref)
    icons["view-refresh"] = (d, b)

    # 7. window-close
    g_close = """
    <circle cx="256" cy="256" r="210" fill="#dc2626"/>
    <line x1="180" y1="180" x2="332" y2="332" stroke="#ffffff" stroke-width="32" stroke-linecap="round"/>
    <line x1="332" y1="180" x2="180" y2="332" stroke="#ffffff" stroke-width="32" stroke-linecap="round"/>
    """
    d, b = action_btn(g_close)
    icons["window-close"] = (d, b)

    # 8. edit-cut
    g_cut = """
    <circle cx="190" cy="330" r="30" fill="none" stroke="#ffffff" stroke-width="16"/>
    <circle cx="322" cy="330" r="30" fill="none" stroke="#ffffff" stroke-width="16"/>
    <line x1="200" y1="304" x2="340" y2="150" stroke="#ffffff" stroke-width="18" stroke-linecap="round"/>
    <line x1="312" y1="304" x2="172" y2="150" stroke="#ffffff" stroke-width="18" stroke-linecap="round"/>
    """
    d, b = action_btn(g_cut)
    icons["edit-cut"] = (d, b)

    # 9. edit-copy
    g_copy = """
    <rect x="160" y="160" width="130" height="150" rx="14" fill="none" stroke="#ffffff" stroke-width="16"/>
    <rect x="210" y="210" width="130" height="150" rx="14" fill="#38bdf8" stroke="#ffffff" stroke-width="16"/>
    """
    d, b = action_btn(g_copy)
    icons["edit-copy"] = (d, b)

    # 10. edit-paste
    g_paste = """
    <rect x="170" y="160" width="172" height="220" rx="18" fill="#ffffff"/>
    <rect x="216" y="130" width="80" height="40" rx="8" fill="#f59e0b"/>
    <line x1="200" y1="230" x2="312" y2="230" stroke="#0f172a" stroke-width="12" stroke-linecap="round"/>
    <line x1="200" y1="270" x2="312" y2="270" stroke="#0f172a" stroke-width="12" stroke-linecap="round"/>
    <line x1="200" y1="310" x2="270" y2="310" stroke="#0f172a" stroke-width="12" stroke-linecap="round"/>
    """
    d, b = action_btn(g_paste)
    icons["edit-paste"] = (d, b)

    # 11. edit-delete
    g_del = """
    <path d="M 180 180 L 190 360 C 190 375, 205 385, 220 385 L 292 385 C 307 385, 322 375, 322 360 L 332 180" fill="none" stroke="#ef4444" stroke-width="18" stroke-linecap="round"/>
    <line x1="150" y1="180" x2="362" y2="180" stroke="#ef4444" stroke-width="18" stroke-linecap="round"/>
    <path d="M 226 180 L 226 150 L 286 150 L 286 180" fill="none" stroke="#ef4444" stroke-width="14" stroke-linecap="round"/>
    """
    d, b = action_btn(g_del)
    icons["edit-delete"] = (d, b)

    return icons

print("icons_status_actions module ready.")
