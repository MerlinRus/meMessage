# Third-party assets

## SF Pro Text

SF-Pro-Text-Regular.otf and SF-Pro-Text-Semibold.otf are Apple fonts, copyright
Apple Inc. They are loaded only inside this personal local application and do
not replace any Windows system font. Apple license terms apply; inclusion does
not grant redistribution rights.

- Apple font information and terms: https://developer.apple.com/fonts/
- Download source: https://github.com/bradleyhodges/SFWindows/tree/main/SF%20Pro

## Apple emoji artwork and Unicode metadata

The local 32-pixel atlas contains Apple emoji artwork. Artwork copyright belongs
to Apple Inc.; the metadata license does not license Apple's artwork.

- Source and artwork notes: https://github.com/iamcal/emoji-data
- Metadata: https://raw.githubusercontent.com/iamcal/emoji-data/master/emoji.json
- Atlas: https://raw.githubusercontent.com/iamcal/emoji-data/master/sheets-clean/sheet_apple_32_clean.png
- Metadata license: assets/emoji/DATA-LICENSE.txt (MIT).

The loaded snapshot supplies Emoji 17.0, 1,916 base entries and 2,030 skin variants.
Only entries with Apple artwork are offered. These files are local; message text
and emoji searches are never sent to a remote asset service.

## Application SVG icons

The pictograms in assets/icons are project-authored SVG paths. They are embedded
in the executable; they do not use emoji glyphs or Apple SF Symbols files.
