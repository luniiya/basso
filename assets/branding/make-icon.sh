#!/bin/sh
# Rebuild basso-icon.png (32x32 pixel art scaled x16 to 512x512) with the exact colours and layout picked.
cd "$(dirname "$0")"
PAL="BG=#0a1030,ROAD=#1c3770,RIM=#334b84,LINE=#070a1b,ROUTE=#86f0df,LETTER=#86f0df,RING=#dffff9,DISC=#2fbfae,ARROW=#ffffff" \
  python3 -I make_icon.py basso-icon.png 4 2 13 1
