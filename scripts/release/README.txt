Starry Journey
==============

A pixel-art night city that builds itself, window by window, under the
stars. Leave it running as a screensaver or as a "meeting starts soon"
display.

Run
---
Double-click starry-journey.exe. Press F for fullscreen and Esc to quit.

You need a graphics card and driver that supports DirectX 12 or Vulkan
(almost any PC from the last ten years).

Windows may warn "Windows protected your PC" because this free program is not
code-signed. Click "More info", then "Run anyway".

Keys
----
  P          Next palette
  R          New city
  E          Switch between Normal and Endless mode
  C          Cycle cloud cover
  W          Cycle rain, snow, or nothing
  D          Toggle the retro display effects
  Space      Pause or resume Endless scrolling
  + / -      Construction speed
  F          Fullscreen
  Tab        Move to the next display
  Esc        Quit

Options
-------
Start it from a terminal with options, for example:

  starry-journey.exe --palette amber --clouds storm
  starry-journey.exe --timer 10m --message "This meeting will start shortly"

Run "starry-journey.exe --help" for the full list.

License
-------
Starry Journey is free software under the GNU General Public License v3.0 or
later (LICENSE). Source code: https://github.com/idgarad/Starry-Journey

It includes the Noto Sans font (NotoSans-OFL.txt) and glyphs from GNU Unifont
(unifont-OFL.txt), both under the SIL Open Font License 1.1, and open-source
Rust libraries listed with their licenses in THIRD-PARTY-LICENSES.txt.
