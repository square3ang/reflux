# Reflux ↔ Fluxer feature-gap matrix

Compared against the official client at `/tmp/opencode/fluxer` (fluxer_app + fluxer_desktop).
Paths: **F/** = `fluxer_app/src/features/`, **D/** = `fluxer_desktop/src/main/`.
Status: **DONE** · **PARTIAL** · **MISSING**. *(low)* = billing/premium or otherwise low priority.
How each gap or limit is worked around: [WORKAROUNDS.md](WORKAROUNDS.md).
Baseline (2026-11-07): 57 DONE · 67 PARTIAL · 148 MISSING (272 rows); after media: 63 · 68 · 141; voice/video moved 4 rows to DONE and 8 from MISSING to PARTIAL. Audit 2026-11-07 (duplicate rows dropped, stale rows fixed): 72 DONE · 85 PARTIAL · 29 MISSING (186 rows); after misc (members page, active now, alt text, overrides, auto-idle, search history, debug tools, offline cache, premium views), merged with the nav and voice agents' rows: 94 DONE · 90 PARTIAL · 2 MISSING; after community/channel settings, merged with the p2p rows: 106 DONE · 79 PARTIAL · 1 MISSING; after Explore load-more: 107 · 78 · 1; after user settings: 116 · 69 · 1; after social3 (threads, forum, user menu, active now streams, OAuth2, handoff, typing, search nav, report, entrance scopes, voice message trim): 127 DONE · 59 PARTIAL · 0 MISSING; after chat3 (nagbars, NSFW, invites, gifv, search scope/dates, date picker kit, multi-file portal picker, pickers, inbox, reactors, system messages): 142 · 44 · 0; after plat4 (notification clicks, badges, Windows deep links, Flatpak autostart, global shortcut hooks, screen-share picker; Windows/macOS now `cargo check` clean via `scripts/cross-check.sh`): 151 · 35 · 0; after med4 (voice message player, saved media, pop-out pool, Quick CSS window, theme folders, i18n msgids, debug JSON): 156 · 30 · 0; after set4 (profile badges, recovery kit, Bluesky, bot username rules, settings search highlight, overview/role crop, invite previews, activity log sentences): 164 · 22 · 0; after live5 (camera / stream audio live-verified, e2ee_capable identify, gifv on macOS): 165 · 21 · 0; after adv5 (screen-share encoder options, cloning switch, reverse image search settings, synced picker prefs, attachment delete, Windows passkeys): 167 · 19 · 0; after sweep7 (message-menu media items and pins/search media sizing moved to PARTIAL, both waiting on `shell/attachments.rs`): 165 · 21 · 0; after win6 (inline video/audio and gifv on Windows through Media Foundation, GStreamer playback test + accurate-seek fix): 167 · 19 · 0; after sweep8 (Advanced: New settings, unread badges, composer buttons; synced look prefs; Fluxerbot notes; theme sync; thread-panel composer menus/tray; activity log links; and more, rows updated): 168 · 18 · 0; message-menu media items and pins/search media fit: 170 · 16 · 0; sweep9 (synced preferences flush queue with base hash + 409 retry, confusable-skeleton search, switcher message links and accounts, Quick CSS colours and variable completions, Advanced in-tab search and Configure dialogs, bot banner preview field, typed community lists, activity log emoji, invite embed badges, mirrored missed-call icon, broader settings-search highlight, localized region names and "Learn more" on the NSFW gate; no status changes): 170 · 16 · 0; cam10 (camera on Windows through Media Foundation and on macOS through AVFoundation, frame rate setting applied): 171 · 15 · 0; sweep12 (quick switcher member search over cached users + gateway op 8, `@@`, mode headers; Message AST tab with embeds and parse-time summary; no status changes): 171 · 15 · 0. After the audit fixes: SkeletonLayoutMemory, screen-share codec choice, Windows WASAPI / macOS ScreenCaptureKit share audio, stream viewer, call menus, multi-account engine, inbox layout: 176 DONE · 16 PARTIAL · 0 MISSING. Agents: update rows you finish. sweep11 (`BadgeListModal` from the invite embed and profile `+N` pills, `UserProfileBadges` in the popout / full profile, Discord template import, crop Height slider + "Skip cropping", skin-tone and twemoji SVG emoji, Quick CSS Ctrl+Space, camera frames dropped at the source; no status changes): 171 · 15 · 0. Agents: update rows you finish. dev13 (Developer tools header menu: local overrides, forced nagbars, mocks, gateway reset/disconnect; no status changes): 171 · 15 · 0. gap15 (composer permission gating + forced no-send/no-attach, announcement Follow barrier, skeleton shimmer/pulse/history fillers, claim modal legacy flow + recovery kit + passkey offer): 173 · 18 · 0. gap14 (account / voice-restore / community nagbars with Fluxer's priority pick, claim account and messaging-paused modals, verification barriers, loading skeletons, mature media gate details, compact unknown message; 5 new rows, Skeletons back to PARTIAL): 172 · 19 · 0. gap17 (crash screen, voice restore camera/screen share, staff-only + Fluxer HQ nagbars, synced nagbar dismissals): 175 · 16 · 0. gap18 (crash screen update check + runtime cleanup; skeleton layout memory still open): 175 · 16 · 0. aud20 (Linux voice devices and call audio through libpulse/PipeWire with cpal fallback; 80 ms playout queue removes underruns; no status changes): 175 · 16 · 0. translate24 (selected-text "Search the web"/"Translate" with built-in and custom providers, synced defaults, Advanced provider settings): 176 · 15 · 0. stream20 (screen-share streaming modes, resolution/fps/bitrate/SVC, device shares, live re-configure, thumbnail refresh; no status changes): 176 · 15 · 0. focus21 (keyboard/focus/accessibility pass: dialogs and popups take and trap focus, Escape closes one layer at a time, Enter and Space activate controls, the custom key set covers F-keys/Space/navigation keys, PTT accepts a modifier alone, the date picker and Favorites are keyboard reachable, textareas type a newline on Enter, and inputs/errors are labelled for screen readers; no status changes): 176 · 15 · 0. misc24 (media viewer rotation; community emoji/sticker search and static/animated sections; privacy review modal, alert and settings gear dot; bounced-email nagbar + replacement modal): 170 · 22 · 0. multi24 (background account engine: a gateway per saved account, aggregated dock/title badges and per-account switcher badges, notifications and inbox activity, instant demote/promote switching): 171 · 21 · 0. viewer25 (stream viewer controls, spectator popout with profiles, buffering/failed/ended states, source-ended modal): 172 · 20 · 0. callview25 (in-call settings gear menus with devices/profile/level/volumes/region/camera/screen share, compact call surface with remembered height + unread chat badge + stream previews + audio-only avatars + keybind, full call layouts/pin/participant popout/stats/fullscreen/HUD, camera preview device/quality/mirror and hide-own-camera confirm): 174 · 18 · 0. shareaudio26 (Windows WASAPI-loopback and macOS ScreenCaptureKit system audio for screen shares, resampled to 48 kHz stereo; "Capture entire system audio" in the picker, defaulted on once; cross-check clean, runtime unverified): 175 · 17 · 0. adv26 (Advanced tab: 39 of Fluxer's 47 direct controls wired to real behaviour with its labels/defaults — text selection, Neko + keep-still, keyboard hints, favorites, help/download rail buttons, GIF auto-send and saved-media favourites, tracking stripping, trust-all links, expression autocomplete, empty-edit attachments, scroll-to-bottom, mark-all-read confirm, muted-channel default, new-device alerts, media buttons, screen-share codec/AV1/HEVC, PiP preview; the 8 impossible ones are documented in WORKAROUNDS): 177 · 15 · 0.


## 1. Settings tabs

### 1.1 User settings (F/user/components/settings_utils/SettingsConstants.tsx; ours settings/)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Profile | DONE | badges section (hide badges, Operator Pass timestamp/count, live preview), local time in the preview card, GIFs cropped frame by frame (no animated WebP); the crop modal has "Skip cropping" when the original already fits (`canSkipOriginalAssetImage`) and saving an untouched full frame keeps the original | F/user/components/modals/tabs/MyProfileTab.tsx |
| Account | DONE | recovery kit (generate/replace, copy, PDF download, print via the system viewer; "Not now" kept per user on this device); security-key PIN asked only when the key wants one | tabs/account_security_tab/* |
| Privacy | DONE | | tabs/PrivacySafetyTab.tsx |
| Connections | DONE | Bluesky connects through browser OAuth; the dialog closes on USER_CONNECTIONS_UPDATE | tabs/LinkedAccountsTab.tsx, F/connection/* |
| Authorized apps | DONE | | tabs/AuthorizedAppsTab.tsx |
| Blocked users | DONE | | tabs/BlockedUsersTab.tsx |
| Devices | DONE | | tabs/DevicesTab.tsx |
| Plutonium/Subscriptions/Operator pass/Gifts/Payments *(low)* | PARTIAL | read-only: Plutonium link card + subscription status, Gifts inventory (copy links); checkout, plan changes, redeem form, Operator pass and Payments are in the browser | tabs/PlutoniumTab.tsx etc. |
| Appearance | DONE | (device prefs in session.json; the ones Fluxer syncs go through synced preferences 1/80/100/112; Active now has no panel to show) | tabs/AppearanceTab.tsx |
| Themes | DONE | "Sync theme across devices" (off keeps a device-local theme); Quick CSS editor: colours, variable completions (typed or Ctrl+Space) (impossible: Quick CSS beyond `:root` variables — Rune has no CSS engine; the rest is counted and reported, so the DOM-targeted selector / `data-flx` / property / snippet completions, hover, swatches, lint and outline have nothing to act on) | F/theme_settings/* |
| Notifications | DONE | | tabs/NotificationsTab.tsx |
| Chat (Messages & media) | DONE | selected-text "Search the web"/"Translate" open the chosen provider in the browser and sync the default through synced preferences; their Advanced provider settings edit built-in and custom engines | tabs/ChatSettingsTab.tsx |
| Voice & video | DONE | settings tab and the mixer chain; in-call gear menus next to mic/speaker/camera/screen share (input/output device, input profile + live level meter, input/output volume, push-to-talk, deafen, camera device/quality/mirror/preview, streaming modes/quality, screen-share settings, voice-region picker for DM calls with the teleport overlay, prioritize speakers, standard-call switch, voice & video settings); impossible: camera background effects — MediaPipe segmentation | tabs/UserVoiceTab.tsx, VoiceSettingsMenus.tsx |
| Accessibility | DONE | (Fluxer's current Motion tab has no keep-animating flags) | tabs/AccessibilityTab.tsx |
| Language & time | DONE | (impossible: OS spellcheck engine — Rune has no native text services; built-in dictionaries + whatlang auto-detect) | tabs/LanguageTab.tsx |
| Keybinds | DONE | custom Toggle mute / Toggle deafen combos and the PTT key are global where a hook backend runs (X11, Windows, macOS); on Wayland the portal's keys are set in the desktop | tabs/KeybindsTab.tsx |
| Desktop app | PARTIAL | minimize to tray (Rune has no minimize event), window position/maximized | tabs/DesktopSettingsTab.tsx |
| Advanced | DONE | 39 of Fluxer's 47 direct controls, with its labels and defaults: text selection, Neko and keep-still, keyboard hints, favorites, help-centre/download rail buttons, GIF auto-send and saved-media favourites, tracking-parameter stripping, trust-all links, expression autocomplete, keep attachments on empty edit, scroll to bottom on send, mark-all-read confirmation, hide muted by default, new-device alerts, media-button toggles, screen-share codec + AV1/HEVC opt-ins, PiP preview; impossible: native title bar and hardware acceleration (Rune is always system-titlebar/GPU), source maps (Rust build), video-seek thumbnails, smooth scrolling, middle-click autoscroll and stay-interactive (Rune input), pre-upload (uploads start at send), connection volume (platform-audio menus); see WORKAROUNDS | tabs/AdvancedSettingsTab.tsx |
| Applications (dev) | DONE | bot banner preview as Fluxer's `ImagePreviewField` | tabs/applications_tab/* |
| Embed debugger (dev) | DONE | | tabs/EmbedDebuggerTab.tsx |
| Design system (dev) | DONE | | tabs/component_gallery_tab/* |
| Settings search | DONE | 300 ms debounce; matches are highlighted in the shared helpers' and the tabs' text; texts with an inline link (`rich()`) aren't, as Rune keeps the element tree private | SettingsConstants.tsx |

### 1.2 Community settings (ours guild/settings.rs)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Overview | DONE | invite page / invite modal previews; icon, banner, invite and embed backgrounds go through the crop modal (banner and backgrounds with the Height slider, down to half height; threshold date uses the DateTimePickerField kit) | guild_tabs/guild_overview_tab/* |
| Roles | DONE | role icons go through the crop modal; members-in-role isn't in Fluxer's RoleEditor | guild_tabs/GuildRolesTab.tsx |
| Moderation | DONE | | guild_tabs/GuildModerationTab.tsx |
| Activity log | DONE | Fluxer's per-action sentences and detail rows; pages load as the list scrolls; users open the full profile, channels / roles copy their ID; custom emoji drawn as images | guild_tabs/GuildAuditLogTab.tsx |
| Emoji | DONE | the upload dialog picks one file (Rune's file dialog returns one path); several upload by dropping them on the drop zone; search filters by name and the list is split into non-animated and animated sections | guild_tabs/GuildEmojiTab.tsx |
| Stickers | DONE | one at a time like Fluxer's drop zone; images contained in a 320px square (`optimizeStickerImage`); search filters by name | guild_tabs/GuildStickersTab.tsx |
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
| Instance picker, login, TOTP, IP auth, register, forgot pw, verify nagbar | DONE | a bounced address asks for a working email (new address + code, no original-address step) | F/auth/* |
| Passkeys | DONE | Windows goes through webauthn.dll (Windows Hello, security keys, phones), Linux and macOS use USB keys through Firefox's CTAP2 stack (impossible: Linux has no platform authenticator; macOS iCloud passkeys need an associated-domains entitlement the RP must list, which a third-party app can't get) | D/Passkeys.ts |
| Reset pw / verify email / recover pages | DONE | `fluxer://` reset, verify, recover, authorize-ip and wasntme links open the web app (Fluxer desktop doesn't route them in-app either) | F/auth/… |
| SSO login | DONE | via the browser handoff | AuthSsoPanel.tsx |
| Browser login handoff | DONE | (the code, its 6-6 grouping, expiry countdown, copy and regenerate; approving a code happens on Fluxer's web login page, not in the desktop client) | BrowserLoginHandoffModal.tsx |
| Multi-account switcher | DONE | every saved account keeps one background gateway (`app/accounts.rs`, Fluxer's `AccountEngineHost`): its `Store` applies dispatches without history fetches or writes, and its mentions/unread feed the dock badge, title, tray dot, account switcher, desktop notifications (its own DND, mutes, notification levels; account name in the title, click switches and opens the channel) and the inbox's "Other accounts" section. Switching demotes/promotes the live gateway and store, so it is instant and the previous account keeps receiving; signing out or removing an account closes only that gateway. Fluxer has no "keep accounts connected" setting (the engine is always on); Reflux has no per-account `browserNotificationsEnabled`, so the device notification toggle applies and account DND/mutes do their own gating, and cross-account "mark as read" is omitted (a write; read-only task) | AccountSwitcherModal.tsx, D/engine/* |
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
| Nagbars (connection, notice, maintenance…) | DONE | `selectVisibleNagbars` (one non-dismissible + one dismissible by priority; the desktop update bar is pinned); maintenance polls Instatus every 60 s flat; price-announcement nagbars need Stripe billing state; Manage/Reactivate open the billing customer portal; desktop-notification nagbar never shows on desktop (as Fluxer; its Enable / confirm work when forced) | app_layout/nagbars/* |
| Account bars: unclaimed (3-day snooze), account limited + "My text didn't go through" help, Visionary MFA | DONE | dismissals are local | UnclaimedAccountNagbar.tsx, AccountLimitedNagbar.tsx, MessagingPausedHelpModal.tsx, VisionaryMfaNagbar.tsx |
| Claim account modal | DONE | username variant (recovery kit over the app after a password is set), the email flow (add email / set password / 6-digit code with resend countdown, then the once-per-device passkey offer), the legacy `EmailClaimAccountModal` (email-change ticket, code, password); none exercised against a live server | ClaimAccountModal.tsx, AuthenticatorOfferModal.tsx |
| Voice session restore nagbar | DONE | snapshot saved while connected (on change + every minute, 5 min decay), shown for the startup snapshot; Reconnect restarts the camera and opens the screen share picker (`restoreLocalMedia`) | VoiceSessionRestoreNagbar.tsx |
| Skeletons | DONE | app skeleton while connecting (fades out over the app), message list fillers before the first page and above / below older or newer pages, profile popout skeleton with its 1.4 s pulse, member list; 3.2 s shimmer (none under reduced motion); `SkeletonLayoutMemory` keeps the last rail/channel/member shapes per account for 24 h and the booting skeleton draws them | F/app/components/skeleton/*, UserProfileLoadingSkeleton.tsx |
| Reopen last location | DONE | | useLastLocationPersistence.ts |
| Window title unread count | DONE | | useFluxerDocumentTitle.ts |
| Toasts | DONE | only used by add-to-dictionary so far | Toasts.tsx |
| Pop-out windows | DONE | (voice pop-outs keyed by call/tile from a pool of 4 windows — Rune can't open windows after start, the oldest gives way; Quick CSS editor pop-out; Fluxer has no chat pop-outs) | PopoutWindow.tsx |
| Error / rate-limit modals | DONE | DM-restricted/content-blocked texts are the modal body; REST waits out 429s up to 10 s (route and global buckets from the body/`X-RateLimit-*` headers) and retries 502/504/507/522/523/524/598/599 and transport errors twice (sends only with a nonce), then maps what is left through Fluxer's modal texts | GenericErrorModal.tsx |

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
| Invite embed | DONE | built-in and instance (`badge_ids`) badges with tooltips (a custom badge's name in 600 over its description); the `+N` pill opens `GuildBadgesModal` (`BadgeListModal`) | InviteEmbed.tsx |
| NSFW gate / blur | DONE | region names localized from CLDR (`assets/regions.json`), US states by name; "Learn more" links when the instance has a help center; geo / account media gates open `MatureMediaGateDetailsModal` from Reveal ("Click for details") | MatureContentChannelGate.tsx, MatureMediaBlurOverlay.tsx |
| Guild members page | DONE | | GuildMembersPage.tsx |
| DM list | DONE | | DirectMessageList.tsx |
| Create DM / group DM | DONE | | CreateDMModal.tsx |
| Group DM management | DONE | | EditGroupModal.tsx |
| Active Now | DONE |  | ActiveNowSidebar.tsx |
| Private channel create step | DONE | | ChannelCreateAccessStep.tsx |
| Duplicate channel | DONE | | ChannelDuplicateModal.tsx |
| Search UI | DONE | jump highlight also tints the matching result (unchecked vs Fluxer) | message_search_bar/* |
| Call system message / system message wording | DONE | English list joining | CallMessage.tsx, SystemMessageUtils.tsx |

### messaging/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Send/edit/delete/reply/react/pin/unread/copy, delete confirm, error modals | DONE | (live-verified against a real instance, `--live=write`: nonce dedupe, edit, react, reply, pin/unpin, mark unread, delete) | |
| Offline queue | DONE | failed text sends stay (Retry/Delete), file sends go back to the composer; Fluxer has no queue-while-disconnected either; transient REST failures are retried (message sends only with their nonce, upload parts 4× with `Retry-After` honoured) before a send is marked failed | MessageQueue.tsx |
| Action bar / context menu | DONE | right-clicking attachment or embed media adds copy (not video) / download / copy link / open link (`MediaMenuData`) | MessageContextMenu.tsx, MediaMenuData.tsx |
| Forward | DONE | | ForwardModal.tsx |
| Saved messages | DONE | (the inbox's Bookmarks tab, bookmark/unbookmark in the message menu) | SavedMessagesPage.tsx |
| Inbox / recent mentions | DONE | | InboxPopout.tsx |
| Who reacted | DONE | filter right-click "Remove reaction" is in the message menu | ReactionTooltip.tsx, MessageReactionsModal.tsx |
| Media viewer | DONE | rotate clockwise / counterclockwise in 90° steps and reset (stills; a native video surface stays upright) | MediaModal.tsx |
| Edit alt text | DONE | single-line field | EditAltTextModal.tsx |
| External link warning | DONE | | ExternalLinkWarningModal.tsx |
| Markdown alerts/tables | DONE | unicode emoji: base sheet, skin-tone variants from their tone's sheet, anything else as twemoji SVG from the static host (rasterised by Rune's `svg` feature) | markdown/renderers/* |
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
| Profile popout | DONE | `UserProfileBadges`: 7 badges (art or caption) on the plate over the header, Fluxer's detail / quote tooltips, links, Plutonium badges open the Plutonium page, `+N` opens `BadgeListModal`, pencil on your own | UserProfilePopout.tsx |
| Full profile modal | DONE | 15 badges inline under the names, same tooltips / `+N` list | UserProfileModal.tsx |
| Change own nickname | DONE | | ChangeNicknameModal.tsx |
| Community member avatars | DONE | in the selected community every avatar uses the member's own; profile banners too | F/user/components/profile/* |
| User context menu | DONE |  | UserContextMenu.tsx |

### guild
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Create/join, header popout, context menu, privacy modal, leave | DONE | "Import Discord template" (`TemplateImportForm`): link → direct lookup of the public template endpoint (desktop app's `OtherPlatformTemplateIpc`; 404 / 429 errors, else falls back to pasting the JSON), valibot-equivalent validation and stripping, template preview with counts, `POST /guilds` with `template`; never run against Discord or a server (no network writes) | |
| Notification modal | DONE | unread badges (with Advanced's switch) and the per-community reply mention preference | GuildNotificationSettingsModal.tsx |
| Edit community profile | DONE | | MyProfileTab.tsx |
| Delete my messages | DONE | | useDeleteMyMessagesInGuild.tsx |
| Preview nagbar | DONE | (Join / Stop previewing; visiting permissions in `compute_permissions`) | GuildPreviewNagbar.tsx |
| Community nagbars: staff-only, MFA requirement, invites disabled (raid text, re-enable confirm), Fluxer HQ membership CTA | DONE | slots shared with the top bars like `availableGuildNagbarSlots`; dismissals sync through synced preferences field 60 (`nagbars`) | GuildLayout.tsx |
| Verification barriers (composer) | DONE | timeout, paused messaging, account tier gates (add email / confirm / set password, with the claim modal), account too new (5 min), member for 10 min, account limited; countdowns tick each second. Fluxer has no phone or membership-screening gate (levels cap at HIGH) | GuildVerification.ts, BarrierComponents.tsx |
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
| Join/leave, panel, mute/deafen, rings, devices, HW encode | DONE | Linux device lists come from the PulseAudio server (PipeWire sinks/sources with their descriptions, default entry, re-listed by the 10 s hotplug poll) and play/capture run on the selected sink/source; cpal/ALSA is the fallback when no server answers. Playout queues 80 ms server-side (40 ms prefill), so `pw-top` shows no xruns under load | |
| Screen share | PARTIAL | in-app thumbnails on X11/Windows, refreshed every 2 s while the picker is open; macOS uses the system picker; Wayland: impossible — the xdg-desktop-portal ScreenCast API can't enumerate sources (the compositor's own dialog, with its own previews, picks one) and libwebrtc's PipeWire capturer only goes through it. Streaming modes (Gaming 1440p60 / Screen share 1080p30 / Custom, clamped to 720p30 without `feature_higher_video_quality`), resolution 480p–1440p–Source, 15/30/60 FPS, Fluxer's automatic bitrate table (instance `max_screen_share_bitrate`) plus manual caps, content hint → degradation preference, `Devices` tab (camera published as a screen share), Change stream, and live re-configure of a running share (resolution/fps/bitrate/audio restart the capture); an SVC mode publishes VP9 (software) so the layers actually apply. Live-verified with `--live=voice`: 720p30 1280×720/29.5 fps/5.3 Mbps, 480p15 852×480/15.0 fps/0.5 Mbps, 1080p60 1920×1080/56–58 fps/5–6 Mbps, 8 Mbps cap honoured, Screen share preset 1920×1080/30, VP9 single-layer 1280×720/15 fps (libvpx), device share 1280×720/15 (NVENC), camera 640×360/20 and 1280×720 (USB capture limit ~8 fps in a debug build) | ScreenSharePickerModal.tsx |
| Voice channel view | DONE | text chat under the compact call surface: per-channel height remembered in session.json (drag handle and arrow keys, 112–480 px), chat pill with the unread badge ("Show chat with N unread"), live stream previews from the Active Now thumbnail cache with Watch stream fallback, audio-only avatar layout, collapsible/expandable view, and `voice_toggle_compact_call_view` (Ctrl+Shift+V) | F/channel/components/channel_view/* |
| Watch stream | DONE | viewer header with the streamer's avatar and "{name}'s screen", the resolution + measured FPS + LIVE pill, the spectator popout (avatars, count, one row per connection with desktop/mobile icon, click opens the profile), per-stream volume/mute into the mixer, a Stream settings menu (volume, mute, info, pin, fullscreen, pop-out, picture-in-picture, try again, stop), pin = focus the stream in the call view, fullscreen, pop-out window + PiP. Body states follow `VoiceParticipantTileScreenShareStateValue`: 9-dot `ScreenShareBufferingFrame` (full, and the corner variant over a dimmed last frame when a live stream stalls), "Watching failed :(" with `error code {code}` (-2303 first frame / -2304 republish, 15 s), "Stream ended" + Stop watching, own preview = "Preview paused to save resources" + "Your stream is still live". A display/window source that stops ends the local share with Fluxer's "Screen share ended" modal (camera-device shares can't tell; `publish_test_screen` has no end signal). The viewer is its own surface, not a focus tile: pin sets the call view's focused participant | WatchStreamOverlay.tsx, useScreenShareWatchFailure.ts, StreamSpectatorsPopout.tsx, ScreenShareBufferingFrame.tsx |
| Camera | DONE | one `camera::CameraCapture` per OS: GStreamer on Linux, Media Foundation on Windows (`camera/mf.rs`: `MFEnumDeviceSources`, native mode closest to the camera quality, RGB32 via the source reader), AVFoundation on macOS (`camera/av.rs`: `AVCaptureSession` preset by quality, BGRA data output; asks for camera access on first use, a bundle needs `NSCameraUsageDescription`); device list, preview, publish, Fluxer's presets (640×360@20, 1280×720@30, 1920×1080@30 with the encoder's `max_framerate`, 1080p needs `feature_higher_video_quality`). Linux live-verified with `--live=voice` (640×360 at 20 fps; the USB capture card only delivers ~1280×720 at 6–10 fps in a debug build) and `cargo test -p reflux camera::real -- --ignored` on a real V4L2 device; the in-call preview shows the self view with device, quality (480p/720p/1080p, the last gated by `feature_higher_video_quality`) and mirror controls, and "Show my own camera" off has Fluxer's hide-own-camera confirmation with "Don't ask me again"; Windows: wine runs enumeration (no cameras: wine has no MF video capture source) and the clean failure, conversion/mode/pacing unit-tested — not run on a real Windows camera; macOS compile-checked only (`scripts/cross-check.sh`). (impossible: background effects, see Voice & video) | CameraPreviewModal.tsx, HideOwnCameraConfirmModal.tsx |
| Call view grid | DONE | grid/focus/back-to-channel layouts (More options' Grid view + the list button), pin a participant, participant avatar stack and full participant-list popout, connection-stats popout (duration, participants, jitter; packet loss and bitrate appear when the engine reports them), app-window fullscreen, and auto-hiding chrome (pointer idle or leaving) | VoiceCallView.tsx, VoiceCallLayoutPresentationStateMachine.ts |

| DM calls / ringing / P2P | PARTIAL | P2P mesh (op 13, STUN-only) verified by a loopback test only, never against a real peer; conversion watchdog (10 s) and the 16 KB outbound-signal check ported; rollout read once per session; DM calls untested against a server | IncomingCallUI.tsx, VoiceMeshEngine.ts |
| Per-user volume / local mute | DONE |  | VoiceParticipantMenuItems.tsx |
| Noise suppression | DONE |  | | AudioProcessingModal.tsx |
| Voice details popout | DONE | duration, participants, average ping, jitter and the Advanced packet-loss/bitrate rows; packet loss and bitrate stay dashes until the shell has a WebRTC stats handle | VoiceDetailsPopout.tsx |
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
| Search UI chips/suggestions, history | DONE | desktop has token styling (Rune spans: no chip border); confusable-skeleton matching (`unicode-confusables.txt`); values the filter can't use show as unapplied | F/search/* |
| Quick switcher recents | DONE | previous, mentions (newest mention first) and unread channels; message links and other accounts; people are every cached user, `@` narrows to friends and members of the open community with a debounced gateway member request (op 8, 25, presences), `@@` searches all users; mode headers | QuickSwitcher.tsx |
| Custom keybinds, cheatsheet modal, keyboard mode | DONE | | F/input/* |
| Custom themes, Quick CSS, theme sharing | PARTIAL | see Themes row | F/theme*/* |
| i18n coverage, plurals, RTL, locale dates | DONE | (impossible: RTL layout mirroring — Rune has no layout direction, bidi text only; text only Reflux has, e.g. gateway/passkey errors, has no translation in Fluxer's catalogs and stays English) | F/i18n/* |
| Reduced motion, screen reader announcements | DONE | | F/accessibility/* |
| Streamer mode | DONE | | F/streamer_mode/* |
| Debug modals, experiments, account engine cache, premium *(low)* | PARTIAL | debug user/channel/community/member/message (Message AST: content, embed title/description/fields with parse times and the Summary card) + a read-only inspector (state, settings, `/experiments`); the Developer tools header menu (developer mode or staff: state summary, active overrides, general/premium/mocking overrides, forced nagbars, runtime utilities incl. gateway reset/disconnect and Trigger React crash, which opens the crash screen (`ErrorFallback`: checks for updates first ("Update app" when one is found, "Reload app" otherwise; no feed = none found), leaves voice and closes the gateway until Reload; Copy stack trace / Reset app data; panics in poll/view show it at once, other panics on the next launch); DOM-only and barrier items left out, see WORKAROUNDS); offline cache (last READY + 50 msgs × 30 channels per account); premium see §1.1 (JSON is syntax-highlighted; Fluxer's experiment switches live in Advanced, ported); billing/checkout stays in the browser | |

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
| Screen-share picker / app audio | DONE | listed sources with thumbnails (X11/Windows, refreshed every 2 s), own window last, Wayland portal Applications/Entire screen, streaming modes and resolution/fps, the Devices tab, Linux PipeWire app audio, and now the whole system mix on Windows (WASAPI loopback of the default render device, `audio_share/windows.rs`) and macOS (ScreenCaptureKit `SCStream` audio, `audio_share/macos.rs`), listed as "Capture entire system audio" and turned on once like Fluxer's `screenShareAudioDefaultOnMigratedV1`; both are resampled/converted to 48 kHz stereo for `ScreenshareAudio`, and a failed start logs and leaves a video-only share. Windows: Wine runs the loopback path (start + 10 ms frames through Wine's null render device) and the format/resample conversion is unit-tested, but not run on a real Windows box; macOS compile-checked only (`scripts/cross-check.sh`), not run on a Mac | ScreenSharePickerModal.tsx |
| App menu (macOS) | DONE | built on Rune standard menus; not verified on a Mac | D/Menu.ts |
| File downloads | DONE | | D/FileDownloads.ts |
| Copy image | DONE | | D/MediaClipboard.ts |
| Streamer-mode detection, theme folder, troubleshooting *(low)* | DONE | (OBS/XSplit detection; watched theme folders polled every 2 s like Fluxer's fallback; impossible: Help › Troubleshooting — it's a native app-menu item, Rune's native menus are macOS-only, Reflux has no app menu, Rune always renders on the GPU and there is no web view to reload) | |
