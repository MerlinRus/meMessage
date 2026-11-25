# meMessage / iPhone Bridge

Native Windows desktop app for an iPhone paired over Bluetooth. Rust + egui/eframe
with OpenGL, Win32 Bluetooth and Winsock. No Mac, Electron, embedded browser,
Apple ID, cloud storage or phone-side app is required for text messaging.

## Purpose

Read and send messages from a lightweight Windows client. Match the restrained
macOS Messages visual direction in DESIGN.md. Operate mode: precise alignment,
readable text, compact controls and responsive input take precedence over decoration.

## Transport and product truth

MAP/OBEX carries text and notifications; PBAP imports names and telephone numbers.
Manual local names override imported names. iPhone selects iMessage or SMS.
Acceptance by iPhone does not prove delivery, and a lost response must never trigger
an automatic resend. In the verified setup the sent-folder listing is empty.

Images are unavailable in the current iPhone MAP transport: even Attachment=1
returns a text placeholder. Do not invent image data, sent status, receipts, typing
indicators, online presence or unsupported group behavior. Do not infer gender.

## Local data

History, contact names/photos, settings and drafts are protected with Windows DPAPI
under %LOCALAPPDATA%/meMessage, or data/ when portable.flag exists. Live
messages are never replaced with preview data. Preview uses synthetic conversations
and cannot send or access Bluetooth. Data, local builds, diagnostics and external
font/art assets are excluded from the public repository.

## Quality

Keep worker I/O outside the UI thread. Preserve drafts on dialogue changes and
failed sends. Respect Windows animation settings and stop repainting when settled.
Use stable widget identities and the shared geometry; measure text at the exact
editor width. Verify the real rendered component, not a duplicate implementation.

## Everyday operation

Pinned/muted chat preferences share encrypted Settings and survive PBAP updates.
Search returns individual locally stored messages and jumps to the selected bubble;
it does not promise an iPhone archive. Results are cached and revealed in batches.
Notifications respect global preferences, contact mute, local read and active chat;
hidden previews never include message text. Initial sync and old pages stay quiet.
Reconnect uses bounded backoff and a native resume event, never retries a message.
Failed before submission and unknown acknowledgement are distinct persistent states.
Manual retry requires confirmation; accepting a retry preserves the next draft.
Native tray has Open/Settings/Exit; hiding requires a working tray. Close/exit handling
must run in App::logic as hidden/occluded windows may not execute App::ui.
Autostart is opt-in, per-user, quotes the executable path and starts in the tray;
the single-instance lock prevents concurrent writers in current builds.
