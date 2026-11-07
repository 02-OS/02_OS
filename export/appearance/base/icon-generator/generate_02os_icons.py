import os
import sys

def make_svg(defs, body, width=512, height=512):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="100%" height="100%">
  <defs>
    <!-- Ambient Drop Shadow -->
    <filter id="card-shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="18" stdDeviation="20" flood-color="#000000" flood-opacity="0.48"/>
    </filter>
    <filter id="inner-shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="6" stdDeviation="8" flood-color="#000000" flood-opacity="0.32"/>
    </filter>
    <filter id="neon-glow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="14" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>

    <!-- Glass Specular Rim Highlight -->
    <linearGradient id="specular-rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.45"/>
      <stop offset="25%" stop-color="#ffffff" stop-opacity="0.15"/>
      <stop offset="70%" stop-color="#ffffff" stop-opacity="0.04"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.12"/>
    </linearGradient>

    <!-- Top Gloss Sheen Overlay -->
    <linearGradient id="top-gloss" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.30"/>
      <stop offset="40%" stop-color="#ffffff" stop-opacity="0.06"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.0"/>
    </linearGradient>

    {defs}
  </defs>
  {body}
</svg>'''

def app_squircle(bg_gradient_id, art):
    return f'''
  <!-- Card with Drop Shadow -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="url(#{bg_gradient_id})" filter="url(#card-shadow)"/>
  
  <!-- Subtle Top Glass Sheen -->
  <path d="M 36 140 C 36 82, 82 36, 140 36 L 372 36 C 430 36, 476 82, 476 140 L 476 210 C 360 240, 150 240, 36 210 Z" fill="url(#top-gloss)"/>
  
  <!-- Hero Foreground Symbol -->
  <g filter="url(#inner-shadow)">
    {art}
  </g>
  
  <!-- Glass Specular Rim Border -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="none" stroke="url(#specular-rim)" stroke-width="2.5"/>
'''

print("Helper templates compiled.")
