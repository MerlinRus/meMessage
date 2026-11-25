# Everyday messaging: implementation and acceptance

Scope approved in chat: reliable reconnect, honest send state, searchable local history,
tray/autostart/unread count, pinned and muted chats, private notifications, user-flow QA.
Keep Rust/egui, SF Pro and existing Messages geometry. No automatic message resend,
delivery claims, unavailable phone archive, photos or forced iMessage/SMS routing.

## Ordered work

1. Connection recovery: bounded reconnect backoff with immediate manual retry; a long
   pause resumes polling promptly; transport failures do not erase drafts/history.
   Verify scheduler tests and connection UI scenarios.
2. Sending: distinguish failures before submission from unknown acknowledgement;
   interrupted Pending becomes Unconfirmed on load; every failed/uncertain bubble
   stays actionable after another message. Manual retry requires an explicit action
   and confirmation for uncertain outcomes. Verify synthetic send scenarios.
3. Search: contact/phone and message results, newest first, click jumps to exact
   message without deleting draft; clear/Escape restores normal list. Verify real UI.
4. Chats/privacy: encrypted backward-compatible pin/mute preferences; pinned chats
   sort first; context menus copy number and change preferences. Muted/read messages
   never generate new toasts; preview visibility is configurable. Verify policy tests.
5. Windows integration: real tray Open/Exit, hide-on-close only with working tray,
   unread tray/taskbar count, opt-in per-user autostart, hidden start, and explicit
   exit without losing drafts. Verify platform operations and native preview.
6. Polish: compact/full windows, fractional DPI, long names/messages/errors, emoji,
   multiline, search, switch during send, settings/notifications, background read rules.

## Validation

`cargo fmt --check`, `cargo test --locked` (outside sandbox for DPAPI),
`cargo clippy --locked --all-targets -- -D warnings`, `cargo build --release --locked`.
Synthetic UI tests never send messages. Native QA uses isolated preview. Document
which hardware scenarios still need a real incoming message or physical Bluetooth.
Do not put tests back in GitHub Actions; do not publish personal data or screenshots.
