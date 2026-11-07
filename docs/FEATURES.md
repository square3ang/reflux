# Reflux ↔ Fluxer feature-gap matrix

Compared against the official client at `/tmp/opencode/fluxer` (fluxer_app + fluxer_desktop).
Paths: **F/** = `fluxer_app/src/features/`, **D/** = `fluxer_desktop/src/main/`.
Status: **DONE** · **PARTIAL** · **MISSING**. *(low)* = billing/premium or otherwise low priority.
How each gap or limit is worked around: [WORKAROUNDS.md](WORKAROUNDS.md).
Baseline (2026-11-07): 57 DONE · 67 PARTIAL · 148 MISSING (272 rows); after media: 63 · 68 · 141; voice/video moved 4 rows to DONE and 8 from MISSING to PARTIAL. Audit 2026-11-07 (duplicate rows dropped, stale rows fixed): 72 DONE · 85 PARTIAL · 29 MISSING (186 rows); after misc (members page, active now, alt text, overrides, auto-idle, search history, debug tools, offline cache, premium views), merged with the nav and voice agents' rows: 94 DONE · 90 PARTIAL · 2 MISSING; after community/channel settings, merged with the p2p rows: 106 DONE · 79 PARTIAL · 1 MISSING; after Explore load-more: 107 · 78 · 1; after user settings: 116 · 69 · 1; after social3 (threads, forum, user menu, active now streams, OAuth2, handoff, typing, search nav, report, entrance scopes, voice message trim): 127 DONE · 59 PARTIAL · 0 MISSING; after chat3 (nagbars, NSFW, invites, gifv, search scope/dates, date picker kit, multi-file portal picker, pickers, inbox, reactors, system messages): 142 · 44 · 0; after plat4 (notification clicks, badges, Windows deep links, Flatpak autostart, global shortcut hooks, screen-share picker; Windows/macOS now `cargo check` clean via `scripts/cross-check.sh`): 151 · 35 · 0; after med4 (voice message player, saved media, pop-out pool, Quick CSS window, theme folders, i18n msgids, debug JSON): 156 · 30 · 0; after set4 (profile badges, recovery kit, Bluesky, bot username rules, settings search highlight, overview/role crop, invite previews, activity log sentences): 164 · 22 · 0; after live5 (camera / stream audio live-verified, e2ee_capable identify, gifv on macOS): 165 · 21 · 0; after adv5 (screen-share encoder options, cloning switch, reverse image search settings, synced picker prefs, attachment delete, Windows passkeys): 167 · 19 · 0; after sweep7 (message-menu media items and pins/search media sizing moved to PARTIAL, both waiting on `shell/attachments.rs`): 165 · 21 · 0; after win6 (inline video/audio and gifv on Windows through Media Foundation, GStreamer playback test + accurate-seek fix): 167 · 19 · 0; after sweep8 (Advanced: New settings, unread badges, composer buttons; synced look prefs; Fluxerbot notes; theme sync; thread-panel composer menus/tray; activity log links; and more, rows updated): 168 · 18 · 0; message-menu media items and pins/search media fit: 170 · 16 · 0. Agents: update rows you finish.

## 1. Settings tabs

### 1.1 User settings (F/user/components/settings_utils/SettingsConstants.tsx; ours settings/)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Profile | DONE | badges section (hide badges, Operator Pass timestamp/count, live preview), local time in the preview card, GIFs cropped frame by frame (no animated WebP, no Skip button) | F/user/components/modals/tabs/MyProfileTab.tsx |
| Account | DONE | recovery kit (generate/replace, copy, PDF download, print via the system viewer; "Not now" kept per user on this device); security-key PIN asked only when the key wants one | tabs/account_security_tab/* |
| Privacy | DONE | | tabs/PrivacySafetyTab.tsx |
| Connections | DONE | Bluesky connects through browser OAuth; the dialog closes on USER_CONNECTIONS_UPDATE | tabs/LinkedAccountsTab.tsx, F/connection/* |
| Authorized apps | DONE | | tabs/AuthorizedAppsTab.tsx |
| Blocked users | DONE | | tabs/BlockedUsersTab.tsx |
| Devices | DONE | | tabs/DevicesTab.tsx |
| Plutonium/Subscriptions/Operator pass/Gifts/Payments *(low)* | PARTIAL | read-only: Plutonium link card + subscription status, Gifts inventory (copy links); checkout, plan changes, redeem form, Operator pass and Payments are in the browser | tabs/PlutoniumTab.tsx etc. |
| Appearance | DONE | (device prefs in session.json; the ones Fluxer syncs go through synced preferences 1/80/100/112; Active now has no panel to show) | tabs/AppearanceTab.tsx |
| Themes | DONE | "Sync theme across devices" (off keeps a device-local theme) (impossible: Quick CSS beyond `:root` variables — Rune has no CSS engine; the rest is counted and reported) | F/theme_settings/* |
| Notifications | DONE | | tabs/NotificationsTab.tsx |
| Chat (Messages & media) | DONE | (emoji autocomplete toggle isn't in Fluxer's current tab) | tabs/ChatSettingsTab.tsx |
| Voice & video | DONE | (impossible: camera background effects — Fluxer runs MediaPipe selfie segmentation (`selfie_segmentation_256x256.onnx`) in onnxruntime-web; GStreamer has no person-segmentation element and Reflux has no ML runtime) input/output volume, processing profiles and the selected devices go through our own mixer and mic chain (WebRTC APM echo cancel + AGC, RNNoise) | tabs/UserVoiceTab.tsx, UserVideoTab.tsx |
| Accessibility | DONE | (Fluxer's current Motion tab has no keep-animating flags) | tabs/AccessibilityTab.tsx |
| Language & time | DONE | (impossible: OS spellcheck engine — Rune has no native text services; built-in dictionaries + whatlang auto-detect) | tabs/LanguageTab.tsx |
| Keybinds | DONE | custom Toggle mute / Toggle deafen combos and the PTT key are global where a hook backend runs (X11, Windows, macOS); on Wayland the portal's keys are set in the desktop | tabs/KeybindsTab.tsx |
| Desktop app | PARTIAL | minimize to tray (Rune has no minimize event), window position/maximized | tabs/DesktopSettingsTab.tsx |
| Advanced | DONE | (impossible: hardware acceleration — Rune always renders on the GPU). Done: "New settings" (`addedAt` within 30 days), unread badge customization (community modal radios; channel list and rail follow the levels), composer button switches, the screen-share encoder Configure modal (encoder path, SVC mode, content hint; the hint maps to libwebrtc's degradation preference, since the Rust wrapper has no content-hint setter), the expression cloning-shortcuts switch, reverse image search settings | tabs/AdvancedSettingsTab.tsx |
| Applications (dev) | DONE | bot banner preview is the profile card's | tabs/applications_tab/* |
| Embed debugger (dev) | DONE | | tabs/EmbedDebuggerTab.tsx |
| Design system (dev) | DONE | | tabs/component_gallery_tab/* |
| Settings search | DONE | 300 ms debounce; matches are highlighted in text built by the shared settings helpers (a tab's bare `text()` isn't, as Rune keeps the element tree private) | SettingsConstants.tsx |

### 1.2 Community settings (ours guild/settings.rs)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Overview | DONE | invite page / invite modal previews; icon, banner, invite and embed backgrounds go through the crop modal (threshold date uses the DateTimePickerField kit) | guild_tabs/guild_overview_tab/* |
| Roles | DONE | role icons go through the crop modal; members-in-role isn't in Fluxer's RoleEditor | guild_tabs/GuildRolesTab.tsx |
| Moderation | DONE | | guild_tabs/GuildModerationTab.tsx |
| Activity log | DONE | Fluxer's per-action sentences and detail rows; pages load as the list scrolls; users open the full profile, channels / roles copy their ID; custom emojis show as `:name:` | guild_tabs/GuildAuditLogTab.tsx |
| Emoji | DONE | the upload dialog picks one file (Rune's file dialog returns one path); several upload by dropping them on the drop zone | guild_tabs/GuildEmojiTab.tsx |
| Stickers | DONE | one at a time like Fluxer's drop zone; images contained in a 320px square (`optimizeStickerImage`) | guild_tabs/GuildStickersTab.tsx |
| Explore (discovery listing) | DONE | | guild_tabs/GuildDiscoveryTab.tsx |
| Vanity URL | DONE | | guild_tabs/GuildVanityURLTab.tsx |
| Webhooks | DONE | | guild_tabs/GuildWebhooksTab.tsx, F/webhook/* |
| Members | DONE | opens the members page, like Fluxer | GuildMembersPage.tsx |
| Invites | DONE | | DisableInvitesButton.tsx |
| Bans | DONE | | GuildBansTab.tsx |
| Delete community | DONE | | GuildDeleteModal.tsx |
| Transfer ownership | DONE | | TransferOwnershipModal.tsx |

### 1.3 Channel / thread / forum / group settings (ours guild/channel.rs)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Channel overview | DONE | | channel_tabs/ChannelOverviewTab.tsx |
| Channel permissions | DONE | member search uses `members-search` (REST) | channel_tabs/ChannelPermissionsTab.tsx |
| Channel invites | DONE | | ChannelSettingsModalUtils.ts |
| Channel webhooks | DONE | | channel_tabs/ChannelWebhooksTab.tsx |
| Delete channel | DONE | | ChannelDeleteModal.tsx |
| Forum settings / tags | DONE | | F/forum/components/settings/* |
| Thread settings | PARTIAL | no live write test | F/threads/…/ThreadSettingsModal.tsx |
| Group DM edit | DONE | | modals/EditGroupModal.tsx |

## 2. Feature areas

### auth/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Instance picker, login, TOTP, IP auth, register, forgot pw, verify nagbar | DONE | | F/auth/* |
| Passkeys | DONE | Windows goes through webauthn.dll (Windows Hello, security keys, phones), Linux and macOS use USB keys through Firefox's CTAP2 stack (impossible: Linux has no platform authenticator; macOS iCloud passkeys need an associated-domains entitlement the RP must list, which a third-party app can't get) | D/Passkeys.ts |
| Reset pw / verify email / recover pages | DONE | `fluxer://` reset, verify, recover, authorize-ip and wasntme links open the web app (Fluxer desktop doesn't route them in-app either) | F/auth/… |
| SSO login | DONE | via the browser handoff | AuthSsoPanel.tsx |
| Browser login handoff | DONE | (the code, its 6-6 grouping, expiry countdown, copy and regenerate; approving a code happens on Fluxer's web login page, not in the desktop client) | BrowserLoginHandoffModal.tsx |
| Multi-account switcher | DONE | | AccountSwitcherModal.tsx |
| Sudo verification | DONE | | SudoVerificationModal.tsx |
| Account restriction / age gate / new-device | DONE | audio devices are polled every 10 s (no devicechange event) and grouped by card name | AccountRestrictionScreen.tsx, DateOfBirthGate.tsx, NewDeviceMonitoring.tsx |
| OAuth2 authorize | DONE |  | OAuthAuthorizeModal.tsx |
| Terms acceptance | DONE | | TermsAcceptanceModal.tsx |
| Token in OS keyring | DONE | falls back to the 0600 file when no keyring | D/DesktopAppStorage.ts |

### app / navigation / window / ui
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Guild rail, user area, UI kit, keyboard nav | DONE | | |
| Guild folders | DONE | | GuildFolderItem.tsx |
| Rail drag-reorder / create folder | DONE | | F/app/components/layout/dnd/* |
| Favorites | DONE | (synced_preferences field 40; no base-hash conflict retry) | FavoritesButton.tsx |
| Set custom status | DONE | | CustomStatusModal.tsx |
| Nagbars (connection, notice, maintenance…) | DONE | maintenance polls Instatus every 60 s flat; price-announcement nagbars need Stripe billing state; Manage/Reactivate open the billing customer portal; desktop-notification nagbar never shows on desktop (as Fluxer) | app_layout/nagbars/* |
| Skeletons | DONE | | F/app/components/skeleton/* |
| Reopen last location | DONE | | useLastLocationPersistence.ts |
| Window title unread count | DONE | | useFluxerDocumentTitle.ts |
| Toasts | DONE | only used by add-to-dictionary so far | Toasts.tsx |
| Pop-out windows | DONE | (voice pop-outs keyed by call/tile from a pool of 4 windows — Rune can't open windows after start, the oldest gives way; Quick CSS editor pop-out; Fluxer has no chat pop-outs) | PopoutWindow.tsx |
| Error / rate-limit modals | DONE | DM-restricted/content-blocked texts are the modal body | GenericErrorModal.tsx |

### channel/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Channel list, header, message list, composer, member list, friends, notes, pins, uploads | DONE | pins/search result cards scale media down to fit | |
| Channel-list typing / unread on muted | DONE | | InterfaceTab.tsx |
| Header bell dropdown | DONE | | ChannelNotificationSettingsDropdown.tsx |
| Topic modal | DONE | | ChannelTopicModal.tsx |
| Announcement follow | DONE | | ChannelFollowModal.tsx |
| DM call buttons | DONE | | CallButtons.tsx |
| Compact layout | PARTIAL | impossible in Rune: no text-indent, and a placeholder run of spaces would leak into copied text and break messages that open with a block (fence, quote, list, heading); text wraps under its own start, not 32px under the name | CompactMessageLayout.tsx |
| Welcome sections | DONE | | ChannelWelcomeSection.tsx |
| Blocked message groups | DONE | | BlockedMessageGroups.tsx |
| Typing indicator tiers | DONE | (tier strings >4 typers are English-only) | ChannelTyping.tsx |
| Rich composer (pills, toolbar) | PARTIAL | custom emoji pills show `:name:`: impossible without a Rune change (pill chips take only a vector `Icon` and no pill rect is exposed to overlay the image); toolbar floats over the selection, centred on its caret end (Rune exposes the caret rect, not the range's) | F/lexical/* |
| Emoji / GIF / sticker / memes pickers | DONE | premium upsell *(low)* not shown. Frecency, favorites, skin tone and favorite GIFs sync through synced preferences 20/21/23/25; "Clone to…" sits behind Advanced's switch; reverse image search has engine settings (built-in toggles, custom engines; the default is synced); message media can be deleted | pickers/*, EmojiPickerPopout.tsx |
| Reaction picker | DONE | | emoji_picker/* |
| Composer context menu | DONE | | TextareaContextMenu.tsx |
| Slash commands | DONE | option hints; `/nick` and a failed `/msg` answer with a Fluxerbot "Only you can see this" note, as do DM / unclaimed / safety send blocks | useCommands.ts |
| @everyone confirmation | DONE | | MentionEveryonePopout.tsx |
| Attachments | DONE | video/audio play inline everywhere: GStreamer on Linux (`gst` feature, default; `cargo test -p reflux gst::` plays generated webm/mp4/mp3 end to end: frames, sound, pause, accurate seek, end, replay), Media Foundation on Windows (`mf.rs`; wine-verified with `scripts/mf-wine-test.sh` on local H.264/AAC mp4 and mp3 files — not yet run on a real Windows box, and https streaming through MF's network source is untested), Rune `Player` on macOS (compile-checked only); a failed clip shows "This video can't be played"; pre-signed and multipart uploads live-verified | embeds/attachments/* |
| Embeds | DONE | gifv plays muted and looping (GStreamer on Linux, Media Foundation on Windows, Rune `Player` on macOS; same verification as Attachments), video links play inline through the same player; (impossible: YouTube inline — the embed carries only an iframe URL, no stream, so play opens the browser) | RichEmbed.tsx |
| Invite embed | DONE | custom badge ids aren't drawn (built-in badges only) | InviteEmbed.tsx |
| NSFW gate / blur | DONE | region shown as its ISO code; no "Learn more" link | MatureContentChannelGate.tsx |
| Guild members page | DONE | | GuildMembersPage.tsx |
| DM list | DONE | | DirectMessageList.tsx |
| Create DM / group DM | DONE | | CreateDMModal.tsx |
| Group DM management | DONE | | EditGroupModal.tsx |
| Active Now | DONE |  | ActiveNowSidebar.tsx |
| Private channel create step | DONE | | ChannelCreateAccessStep.tsx |
| Duplicate channel | DONE | | ChannelDuplicateModal.tsx |
| Search UI | DONE | jump highlight also tints the matching result (unchecked vs Fluxer) | message_search_bar/* |
| Call system message / system message wording | DONE | missed-call icon not mirrored; English list joining | CallMessage.tsx, SystemMessageUtils.tsx |

### messaging/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Send/edit/delete/reply/react/pin/unread/copy, delete confirm, error modals | DONE | (live-verified against a real instance, `--live=write`: nonce dedupe, edit, react, reply, pin/unpin, mark unread, delete) | |
| Offline queue | DONE | failed text sends stay (Retry/Delete), file sends go back to the composer; Fluxer has no queue-while-disconnected either | MessageQueue.tsx |
| Action bar / context menu | DONE | right-clicking attachment or embed media adds copy (not video) / download / copy link / open link (`MediaMenuData`) | MessageContextMenu.tsx, MediaMenuData.tsx |
| Forward | DONE | | ForwardModal.tsx |
| Saved messages | DONE | (the inbox's Bookmarks tab, bookmark/unbookmark in the message menu) | SavedMessagesPage.tsx |
| Inbox / recent mentions | DONE | | InboxPopout.tsx |
| Who reacted | DONE | filter right-click "Remove reaction" is in the message menu | ReactionTooltip.tsx, MessageReactionsModal.tsx |
| Media viewer | DONE | | MediaModal.tsx |
| Edit alt text | DONE | single-line field | EditAltTextModal.tsx |
| External link warning | DONE | | ExternalLinkWarningModal.tsx |
| Markdown alerts/tables | DONE | | markdown/renderers/* |
| Code highlighting | DONE | | F/code_highlighting/* |
| Crosspost | DONE | | MessageCrosspostConfirmModal.tsx |

### notification / read_state / typing / presence
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Desktop notifications | DONE | clicks routed on Linux, Windows and macOS (Win/mac compile-checked only) | MessageNotificationPreview.ts |
| Sounds, status select, custom status display | DONE | | |
| Acks | DONE | | NotificationAutoAck.ts |
| Timed mute expiry | DONE | | useMuteSheet.ts |
| Per-channel overrides | DONE | header bell + the notification modal's overrides section | GuildNotificationSettingsModal.tsx |
| Mute duration submenu | DONE | | MuteTargetMenuItem.tsx |
| TTS | DONE | (speaks through the OS voice tool: spd-say/espeak-ng/say/PowerShell) | TextToSpeech.tsx |
| Auto-idle | DONE | | LocalPresence.ts |
| Thread/forum notification levels | DONE | (no live test) | F/threads/* |

### relationship / user
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Friends actions, notes | DONE | | |
| Friend nicknames | DONE | | ChangeFriendNicknameModal.tsx |
| Add friends to group | DONE | | AddFriendsToGroupModal.tsx |
| Profile popout | DONE | | UserProfilePopout.tsx |
| Full profile modal | DONE | | UserProfileModal.tsx |
| Change own nickname | DONE | | ChangeNicknameModal.tsx |
| Community member avatars | DONE | in the selected community every avatar uses the member's own; profile banners too | F/user/components/profile/* |
| User context menu | DONE |  | UserContextMenu.tsx |

### guild
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Create/join, header popout, context menu, privacy modal, leave | DONE | | |
| Notification modal | DONE | unread badges (with Advanced's switch) and the per-community reply mention preference | GuildNotificationSettingsModal.tsx |
| Edit community profile | DONE | | MyProfileTab.tsx |
| Delete my messages | DONE | | useDeleteMyMessagesInGuild.tsx |
| Preview nagbar | DONE | (Join / Stop previewing; visiting permissions in `compute_permissions`) | GuildPreviewNagbar.tsx |
| Role icons | DONE | | RoleIcon.tsx |
| Folder settings | DONE | | GuildFolderSettingsModal.tsx |

### moderation
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Kick / Ban | DONE | | KickMemberModal.tsx, BanMemberModal.tsx |
| Timeout | DONE | | TimeoutMemberModal.tsx |
| Ban details | DONE | | BanDetailsModal.tsx |
| Report | DONE | (payloads match reports.mdx; not submitted live) | ReportFlowModal.tsx |
| Voice moderation | DONE | (community mute/deafen, move, disconnect) | VoiceParticipantMenuItems.tsx |

### voice
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Join/leave, panel, mute/deafen, rings, devices, HW encode | DONE | | |
| Screen share | PARTIAL | in-app thumbnails on X11/Windows; macOS uses the system picker; Wayland: impossible — the xdg-desktop-portal ScreenCast API can't enumerate sources (the compositor's own dialog, with its own previews, picks one) and libwebrtc's PipeWire capturer only goes through it. NVENC live-verified (`--live=voice`: `TR_` sid, 90 frames, SFU receiver reports) | ScreenSharePickerModal.tsx |
| Watch stream | DONE |  | WatchStreamOverlay.tsx |
| Camera | PARTIAL | Linux only (GStreamer; the `gst` feature is Linux-only); background effects impossible (see Voice & video). Live-verified with `--live=voice`: published (`TR_` sid), NVENC frames encoded and the SFU's receiver reports back, self_video echo | CameraPreviewModal.tsx |
| Call view grid | DONE |  | VoiceCallView.tsx |
| DM calls / ringing / P2P | PARTIAL | P2P mesh (op 13, STUN-only) verified by a loopback test only, never against a real peer; conversion watchdog (10 s) and the 16 KB outbound-signal check ported; rollout read once per session; DM calls untested against a server | IncomingCallUI.tsx, VoiceMeshEngine.ts |
| Per-user volume / local mute | DONE |  | VoiceParticipantMenuItems.tsx |
| Noise suppression | DONE |  | | AudioProcessingModal.tsx |
| Voice details popout | DONE | | VoiceDetailsPopout.tsx |
| PiP / pop-out | PARTIAL | PiP = small always-on-top pop-out; Wayland ignores always-on-top | | PiPOverlay.tsx |
| PTT / VAD | DONE | the recorded key is global through the X11 / Windows / macOS hooks; Wayland takes the portal's key | HoldSources.ts |
| Entrance sounds | DONE | (upload untested live) |  | EntranceSoundTrimmerModal.tsx |
| Moderator move / reconnect | DONE | | VoiceReconnectionManager.tsx |
| E2EE indicator | PARTIAL | untestable live: the gateway grants `e2ee_key` only to sessions identifying `e2ee_capable` (we didn't before; fixed) in a guild with the `VOICE_E2EE` feature or under a platform-wide rollout; the test guild lacks it and the server echoes the effective `e2ee_capable: false`. Key derivation checked both ways against libwebrtc's cryptor with livekit-client's PBKDF2 (`reflux-voice/tests/e2ee.rs`) | VoiceE2EEIndicator.tsx |
| Stream audio (Linux) | DONE | live-verified with `--live=voice` (a `pw-play` tone found via pw-dump, `pipewiresrc` → `ScreenshareAudio`, `TR_` sid + SFU reports; fixed appsink clock sync that dropped ~70% of samples). (Apps playing through GStreamer's `pipewiresink` can't be targeted, as with `pw-record --target`) | | AudioSourcePickerLinux.tsx |
| Join confirm / channel full | DONE | | VoiceConnectionConfirmModal.tsx |
| Voice messages | DONE | (Windows has no inline playback; hold-to-record is mobile-only in Fluxer) | VoiceMessageComposerModal.tsx |

### threads / forum
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Threads | DONE | (demo fixtures + store tests; untested against a live server) | F/threads/* |
| Forum channels + tags | DONE | (untested against a live server) | F/forum/* |

### expressions / emoji / gift / invite / discovery / search / input / theme / i18n / a11y / misc
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Animated emoji/avatars/stickers | DONE | | |
| Expression info popout, favorite memes, clone expression | DONE | (favorite/clone sit on the card; Fluxer also has them in the context menu; emoji/sticker slot limits ignore guild-feature rules) | F/expressions/* |
| Gifts *(low)* | PARTIAL | accept modal for gift links (chat + `fluxer://gift/`) with Claim; inventory read-only | F/gift/* |
| Invite: friends list, accept preview, pause | DONE | | F/invite/* |
| Discovery page | DONE | load-more pages via `offset` | DiscoveryPage.tsx |
| Search UI chips/suggestions, history | DONE | desktop has token styling (chips are mobile-only in Fluxer); values the filter can't use show as unapplied | F/search/* |
| Quick switcher recents | DONE | previous, mentions (newest mention first) and unread channels | QuickSwitcher.tsx |
| Custom keybinds, cheatsheet modal, keyboard mode | DONE | | F/input/* |
| Custom themes, Quick CSS, theme sharing | PARTIAL | see Themes row | F/theme*/* |
| i18n coverage, plurals, RTL, locale dates | DONE | (impossible: RTL layout mirroring — Rune has no layout direction, bidi text only; text only Reflux has, e.g. gateway/passkey errors, has no translation in Fluxer's catalogs and stays English) | F/i18n/* |
| Reduced motion, screen reader announcements | DONE | | F/accessibility/* |
| Streamer mode | DONE | | F/streamer_mode/* |
| Debug modals, experiments, account engine cache, premium *(low)* | PARTIAL | debug user/channel/community/member/message + Developer tools (state, settings, `/experiments` read-only); offline cache (last READY + 50 msgs × 30 channels per account); premium see §1.1 (JSON is syntax-highlighted; Fluxer's experiment switches live in Advanced) | |

## 3. Desktop shell
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Tray, close to tray, flash, open links, HW encoder | DONE | | |
| Minimize to tray / start minimized | PARTIAL | WONTFIX (user decision): minimize to tray — raw-window-handle workaround works only on X11/Windows/macOS, not Wayland | D/Window.ts |
| Notifications | DONE | body click focuses + opens the channel on Linux (D-Bus action), Windows (toast `Activated`) and macOS (NSUserNotification click); Win/mac compile-checked only (`scripts/cross-check.sh`), not run on either | D/NotificationsIpc.ts |
| Badge | DONE | `computeAppBadge` (mentions + friend requests, unread dot behind the setting): Unity LauncherEntry, Windows taskbar overlay (Fluxer's `badge-N.ico`, light set only: the dark-badge switch isn't in our Desktop tab), macOS Dock label; Win/mac compile-checked only (`scripts/cross-check.sh`), not run on either | D/UnreadBadge.ts |
| Deep links | DONE | second instance → first over a Unix socket / per-user named pipe (`AllowSetForegroundWindow`); Windows registers `fluxer://` in HKCU (release builds); macOS needs `CFBundleURLTypes` in the bundle and Rune exposes no `openURLs` event, so links there only work as launch args; Win/mac compile-checked only (`scripts/cross-check.sh`), not run on either | D/DeepLinks.ts |
| Autostart | DONE | XDG entry, Flatpak → Background portal (+ state file), HKCU Run, LaunchAgent; the Flatpak path and Win/mac compile-checked only (`scripts/cross-check.sh`), not run on either | D/Autostart.ts |
| Window state persistence | PARTIAL | WONTFIX (user decision): size only — position/maximized need the raw handle and are impossible on Wayland | D/Window.ts |
| Updater | PARTIAL | blocked on a release feed: none is published, so it stays off unless `REFLUX_UPDATE_FEED` (GitHub latest-release JSON) is set; AppImage self-update now requires and verifies the asset's SHA-256 `digest`; Windows/macOS/Flatpak open the release page (Fluxer self-updates those via Velopack / defers to Flathub; no Velopack packaging exists for Reflux) | D/Updater.ts |
| Global shortcuts / PTT | DONE | Fluxer's backend choice: Wayland → GlobalShortcuts portal (keys set in the desktop); X11 → XRecord hook, Windows → `WH_KEYBOARD_LL`, macOS → listen-only CGEventTap (asks for Input Monitoring), all matching the user's own mute/deafen combos and PTT key with press/release; keyboard only (no mouse-button PTT); X11 hook unit-tested, Win/mac compile-checked only (`scripts/cross-check.sh`), not run on either | D/GlobalShortcutsEngine.ts |
| Spellcheck | DONE | OS engine, language auto-detect; menu targets the caret's word | D/Spellcheck.ts |
| Zoom | DONE | | D/Menu.ts |
| Screen-share picker / app audio | DONE | listed sources with thumbnails (X11/Windows), own window sorted last (`isOwnWindow`), Wayland portal limited to Applications / Entire screen (`set-display-media-portal-preference`), Linux app audio; macOS uses ScreenCaptureKit's picker; the Windows DXGI composition guard isn't needed (libwebrtc's own capturer) | D/DisplayMedia.ts |
| App menu (macOS) | DONE | built on Rune standard menus; not verified on a Mac | D/Menu.ts |
| File downloads | DONE | | D/FileDownloads.ts |
| Copy image | DONE | | D/MediaClipboard.ts |
| Streamer-mode detection, theme folder, troubleshooting *(low)* | DONE | (OBS/XSplit detection; watched theme folders polled every 2 s like Fluxer's fallback; impossible: Help › Troubleshooting — it's a native app-menu item, Rune's native menus are macOS-only, Reflux has no app menu, Rune always renders on the GPU and there is no web view to reload) | |
