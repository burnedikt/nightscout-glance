# Prompt: icon and brand image set for Nightscout Glance

Paste everything below the line into an image-capable design agent.

---

You are designing the icon and brand image set for **Nightscout Glance**, a small open-source desktop app. Read the whole brief before producing anything, then deliver every asset in the list at the end.

## What the product is

Nightscout Glance lives in the Windows system tray (a macOS menu-bar version is planned). It shows a child's current blood glucose value from a Nightscout CGM site as a number right in the taskbar, and shows a history graph when you hover over it. The people who use it are parents and caregivers of children with type 1 diabetes, and adults with T1D. They look at it dozens of times a day, often while worried. It is free, has no account and no tracking.

The feeling to aim for: calm, trustworthy, quietly competent. A well-made utility, closer to a native Windows 11 or macOS system app than to a health startup. Reassuring, never alarming, never cute.

## The existing visual idea (keep it, refine it)

The placeholder logo is a rounded square in deep navy with a single smooth glucose trace line rising from lower left to upper right in fresh green, ending in a white dot with a green centre (the "current reading"). A faint horizontal band behind the line hints at the target range. Keep this concept: **one trace line, one current-value dot, a target band**. You may refine the curve, proportions, colours and depth, but do not replace the idea.

## Palette

- Navy background: `#24345C` to `#0E1426` (diagonal gradient, top-left lighter)
- In-range green: `#4ADE80` (on dark), `#17914A` (on light)
- High amber: `#FFC247` (on dark), `#9A6000` (on light)
- Low red: `#FF6B6B` (on dark), `#C42B1C` (on light)
- Neutrals: `#202020` and `#F7F7F7` surfaces, `#1A1A1A` and `#FFFFFF` text

Green is the brand colour. Amber and red are status colours that appear only inside product UI depictions, never in the logo.

## Hard rules

- No blood drops, syringes, needles, finger-prick imagery, hearts, crosses, pills or medical clipart.
- No owl. The Nightscout project uses an owl mascot; this is an independent app and must not imitate Nightscout's logo or imply it is official.
- No text inside the app icon. No letters, no numbers.
- No stock-photo people, no AI-looking faces, no children.
- The icon must stay recognisable at 16 x 16 px: at that size it is essentially "dark rounded square, green line, white dot". Test this by downscaling and simplify until it reads.
- Flat or very subtly lit. No glossy 3D, no heavy drop shadows, no neon glow, no purple-to-pink gradients.
- Typography in any image that contains text: Segoe UI Variable or Inter, semibold for the name, regular for the tagline. Product name is written "Nightscout Glance" (two words, both capitalised).
- Tagline, when used: "Glucose at a glance, right in your taskbar."

## Deliverables

Produce each of these as a separate file, with the exact file names and sizes given. Use transparent backgrounds where stated.

1. **App icon master**: `icon-1024.png`, 1024 x 1024, transparent outside the rounded square. Corner radius about 23 % of the width.
2. **App icon, small-size variant**: `icon-small-256.png`, 256 x 256, simplified for 16 to 32 px use: thicker line, larger dot, no target band, no gradient noise. Also export this variant downscaled as `icon-16.png`, `icon-24.png`, `icon-32.png`, `icon-48.png`, each pixel-fitted (crisp edges, no blur).
3. **Windows ICO**: `icon.ico` containing 16, 20, 24, 32, 48, 64 and 256 px, using the small variant up to 48 px and the master above that.
4. **macOS icon**: `icon-macos-1024.png`, 1024 x 1024, the same design on Apple's rounded-rectangle (squircle) grid with the standard margin, ready to convert to `.icns`.
5. **Monochrome glyph**: `glyph.svg`, a single-colour vector of the trace line and dot only (no background), stroke-based, usable as a macOS menu-bar template image and as a favicon mask. Must work in pure black and pure white.
6. **Favicon**: `favicon.svg` (the full-colour icon as vector) and `favicon-32.png`.
7. **Horizontal lockup**: `logo-horizontal-dark.svg` and `logo-horizontal-light.svg`, icon on the left, "Nightscout Glance" wordmark on the right, for dark and light backgrounds. Transparent background.
8. **Social preview / Open Graph image**: `og-image.png`, 1280 x 640. Navy background, the icon, the product name, the tagline, and a simple stylised depiction of a taskbar corner showing a glucose number with a small graph card floating above it. Keep at least 80 px of safe margin on every side.
9. **Landing-page hero background**: `hero-bg-dark.png` and `hero-bg-light.png`, 2400 x 1400, an abstract, very quiet composition built from a soft target band and one or two faint trace lines, leaving the centre calm enough for text and a screenshot to sit on top. Low contrast.
10. **README banner**: `banner.png`, 1600 x 400, lockup centred on navy.

## Output format

Return the files plus a short note listing, for each one, its file name, pixel size and background (transparent or colour). If you cannot produce true vector files, say so explicitly and deliver 4x-resolution PNGs with transparent backgrounds instead, so they can be traced later. Before finishing, view the 16 px icon at actual size and confirm it is still legible; if it is not, revise the small variant and say what you changed.
