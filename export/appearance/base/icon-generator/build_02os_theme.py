import os
import sys

def svg_doc(defs, body, width=512, height=512):
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="100%" height="100%">
  <defs>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="18" stdDeviation="20" flood-color="#000000" flood-opacity="0.45"/>
    </filter>
    <filter id="folder-shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="14" stdDeviation="16" flood-color="#000000" flood-opacity="0.40"/>
    </filter>
    <filter id="pocket-shadow" x="-15%" y="-15%" width="130%" height="130%">
      <feDropShadow dx="0" dy="8" stdDeviation="10" flood-color="#000000" flood-opacity="0.30"/>
    </filter>
    <filter id="glyph-shadow" x="-25%" y="-25%" width="150%" height="150%">
      <feDropShadow dx="0" dy="6" stdDeviation="8" flood-color="#000000" flood-opacity="0.35"/>
    </filter>
    <filter id="glow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="12" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>

    <linearGradient id="specular-rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.45"/>
      <stop offset="25%" stop-color="#ffffff" stop-opacity="0.15"/>
      <stop offset="70%" stop-color="#ffffff" stop-opacity="0.04"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.12"/>
    </linearGradient>

    <linearGradient id="top-gloss" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.28"/>
      <stop offset="40%" stop-color="#ffffff" stop-opacity="0.05"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.0"/>
    </linearGradient>

    {defs}
  </defs>
  {body}
</svg>'''

def app_card(bg_id, glyph):
    return f'''
  <!-- Squircle Base with Ambient Shadow -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="url(#{bg_id})" filter="url(#shadow)"/>
  
  <!-- Subtle Top Glass Sheen -->
  <path d="M 36 140 C 36 82, 82 36, 140 36 L 372 36 C 430 36, 476 82, 476 140 L 476 210 C 360 240, 150 240, 36 210 Z" fill="url(#top-gloss)"/>
  
  <!-- Center Hero Artwork -->
  <g filter="url(#glyph-shadow)">
    {glyph}
  </g>
  
  <!-- Specular Rim Border -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="none" stroke="url(#specular-rim)" stroke-width="2.5"/>
'''

def folder_card(badge=""):
    defs = '''
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
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.55"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.1"/>
    </linearGradient>
    '''
    body = f'''
  <!-- Back Folder Body -->
  <path d="M 64 128 C 64 104, 80 88, 104 88 L 196 88 C 214 88, 228 98, 240 112 L 262 138 C 272 150, 286 158, 302 158 L 408 158 C 432 158, 448 174, 448 198 L 448 396 C 448 420, 432 436, 408 436 L 104 436 C 80 436, 64 420, 64 396 Z" fill="url(#f-back)" filter="url(#folder-shadow)"/>
  
  <!-- Inner White Paper Page Peeking Out -->
  <path d="M 96 140 C 96 126, 108 114, 122 114 L 390 114 C 404 114, 416 126, 416 140 L 416 260 L 96 260 Z" fill="#f8fafc" opacity="0.92"/>
  <line x1="130" y1="144" x2="230" y2="144" stroke="#cbd5e1" stroke-width="4" stroke-linecap="round"/>
  <line x1="130" y1="160" x2="350" y2="160" stroke="#cbd5e1" stroke-width="3" stroke-linecap="round"/>
  
  <!-- Front Folder Pocket -->
  <g filter="url(#pocket-shadow)">
    <path d="M 52 196 C 52 176, 68 160, 88 160 L 424 160 C 444 160, 460 176, 460 196 L 460 396 C 460 422, 440 442, 414 442 L 98 442 C 72 442, 52 422, 52 396 Z" fill="url(#f-front)"/>
    <path d="M 52 196 C 52 176, 68 160, 88 160 L 424 160 C 444 160, 460 176, 460 196" fill="none" stroke="url(#f-rim)" stroke-width="3"/>
  </g>
  
  <!-- Folder Context Badge -->
  {badge}
    '''
    return svg_doc(defs, body)

print("Card and folder functions ready.")
