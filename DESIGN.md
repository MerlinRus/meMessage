# Messages for Windows

## Visual authority

The user explicitly requested the design of Messages on macOS. This is a faithful
reconstruction, not a new brand direction. Reference: Apple's macOS Tahoe guide:
https://support.apple.com/en-sg/guide/messages/welcome/14.0/mac/26

Official visual reference:
https://help.apple.com/assets/694059163A98DA7C77096BB3/6940591877CC4A95400B60E4/en_GB/e9371ec3ed11ece50f47745d10c61c88.png

## Surface and composition

Operate mode. Two panes; 294-DIP sidebar (264 at minimum width), 6-DIP inset and 18-DIP corners;
white thread canvas. Compact 74-DIP header with compose at left, centered avatar
and contact capsule, unavailable video control at right. No oversized title.
Traffic-light controls close/minimize/maximize the actual Windows window.

Sidebar: soft gray search pill, contact avatars, name/time and two preview lines.
Selection uses macOS blue with white text. No fabricated pinned people or photos.
Thread: gray received messages, blue outgoing, 17-DIP bubbles, small tail on the
last bubble of a group. Date labels appear at conversation gaps. Footer is a
rounded text composer with plus, send, and emoji controls.

## Tokens

- Content: #FFFFFF; sidebar: #F5F5F7; search: #EAEAED.
- Primary text: #1D1D1F; secondary: #747478.
- Accent / outgoing: #0783FF; received: #E9E9EB.
- Typography: local Apple SF Pro Text Regular / Semibold; 13-DIP UI,
  14-DIP message body, 10–11-DIP metadata.
- Spacing: 2-DIP message grouping, 9-DIP group separation, 20-DIP thread inset.
- Message padding: 11 horizontal / 5 vertical; text is measured once and reused for rendering.
- Conversation rows: 68 DIP. Panel geometry is centralized in src/layout.rs.
- Apple Emoji 17 atlas: 3,946 bases and variants; color glyphs preserve original Unicode.

## Fidelity limits

The app runs Rust/egui on Windows with local SF Pro. Native macOS vibrancy/Liquid Glass,
font rasterization, macOS window behavior, and Apple-only messaging features are not
pixel-identical. Native Windows resize, keyboard focus, and text selection work.
Use user-confirmed behavior for status: 'Передано iPhone', never invented receipts.

## Interaction and verification

Composer text and its placeholder share a centered, margin-free editor inside a
40–128 DIP capsule. Width measurement uses the same inset as the editor. Longer
input scrolls inside the capsule, preserving neighboring controls and the thread.

Motion uses critically damped springs with a 0.4 s response for composer resizing,
0.96→1 scale / 8 DIP settling / short opacity entrance on new messages, and a
120 ms dialogue fade. Menus use 160 ms native egui fading. Initial history is not
replayed as new messages. Motion stops requesting frames when settled and respects
Windows' animation setting. Keyboard shortcuts remain immediate.

Contact aliases appear in the sidebar, header, search and Windows notifications;
the phone number remains available in contact details. Aliases override imported
PBAP names and survive message refreshes. No gender inference or filtering.

Enter sends; Shift+Enter inserts a newline. Ctrl+N creates a message, Ctrl+F searches.
Drafts survive conversation changes. Bluetooth work runs outside the UI thread;
only sends briefly lock the recipient and draft. Search and filters affect real data.
Verify preview at normal and minimum sizes, keyboard navigation, text wrapping,
real iPhone receive, and no unsolicited sends during testing.
