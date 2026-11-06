# Reflux ↔ Fluxer feature-gap matrix

Compared against the official client at `/tmp/opencode/fluxer` (fluxer_app + fluxer_desktop).
Paths: **F/** = `fluxer_app/src/features/`, **D/** = `fluxer_desktop/src/main/`.
Status: **DONE** · **PARTIAL** · **MISSING**. *(low)* = billing/premium or otherwise low priority.
Baseline (2026-11-07): 57 DONE · 67 PARTIAL · 148 MISSING (272 rows); after media: 63 · 68 · 141. Agents: update rows you finish.

## 1. Settings tabs

### 1.1 User settings (F/user/components/settings_utils/SettingsConstants.tsx; ours settings/)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Profile | PARTIAL | interactive crop (auto centre-crop), timezone, badges | F/user/components/modals/tabs/MyProfileTab.tsx |
| Account | PARTIAL | add passkey (no WebAuthn registration), recovery kit | tabs/account_security_tab/* |
| Privacy | DONE | | tabs/PrivacySafetyTab.tsx |
| Connections | PARTIAL | Bluesky (browser OAuth) | tabs/LinkedAccountsTab.tsx, F/connection/* |
| Authorized apps | DONE | | tabs/AuthorizedAppsTab.tsx |
| Blocked users | DONE | | tabs/BlockedUsersTab.tsx |
| Devices | DONE | | tabs/DevicesTab.tsx |
| Plutonium/Subscriptions/Operator pass/Gifts/Payments *(low)* | MISSING | | tabs/PlutoniumTab.tsx etc. |
| Appearance | PARTIAL | cozy/compact, spacing, hide avatars, font size, typing in channel list, unread on muted (zoom + streaming privacy done) | tabs/AppearanceTab.tsx |
| Themes | PARTIAL | legacy dark, system, custom colours/fonts, Quick CSS, sharing | F/theme_settings/* |
| Notifications | PARTIAL | unread badge toggle, per-sound toggles, custom sounds, TTS | tabs/NotificationsTab.tsx |
| Chat (Messages & media) | MISSING | embeds/link previews/reactions/spoilers display, input behaviour | tabs/ChatSettingsTab.tsx |
| Voice & video | PARTIAL | volumes, mic test, processing, PTT/VAD, entrance sound, camera, stream quality | tabs/UserVoiceTab.tsx, UserVideoTab.tsx |
| Accessibility | MISSING | reduced motion, animated emoji, saturation, link underline, keyboard mode | tabs/AccessibilityTab.tsx |
| Language & time | PARTIAL | auto-detect language, OS spellcheck engine, add-a-word field | tabs/LanguageTab.tsx |
| Keybinds | PARTIAL | custom keybinds, recorder, PTT key (global shortcuts toggle in Desktop tab) | tabs/KeybindsTab.tsx |
| Desktop app | PARTIAL | minimize to tray (Rune has no minimize event), window position/maximized | tabs/DesktopSettingsTab.tsx |
| Advanced | PARTIAL | action bar toggles, double-click edit, encoder options, hide blocked, hw accel, auto-update | tabs/AdvancedSettingsTab.tsx |
| Applications (dev) | MISSING | | tabs/applications_tab/* |
| Embed debugger (dev) | MISSING | | tabs/EmbedDebuggerTab.tsx |
| Design system (dev) | PARTIAL | only `--gallery` | tabs/component_gallery_tab/* |
| Settings search | PARTIAL | sections/subtabs | SettingsConstants.tsx |

### 1.2 Community settings (ours guild/settings.rs)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Overview | PARTIAL | splashes, detached banner, history threshold, advanced | guild_tabs/guild_overview_tab/* |
| Roles | PARTIAL | role icon, members-in-role, drag reorder | guild_tabs/GuildRolesTab.tsx |
| Moderation | DONE | | guild_tabs/GuildModerationTab.tsx |
| Activity log | PARTIAL | filters, paging, change details | guild_tabs/GuildAuditLogTab.tsx |
| Emoji | PARTIAL | rename, bulk upload | guild_tabs/GuildEmojiTab.tsx |
| Stickers | PARTIAL | bulk upload, image crop | guild_tabs/GuildStickersTab.tsx |
| Explore (discovery listing) | DONE | | guild_tabs/GuildDiscoveryTab.tsx |
| Vanity URL | DONE | | guild_tabs/GuildVanityURLTab.tsx |
| Webhooks | PARTIAL | editor always returns to the guild tab | guild_tabs/GuildWebhooksTab.tsx, F/webhook/* |
| Members | PARTIAL | filters, paging | GuildMembersPage.tsx |
| Invites | DONE | | DisableInvitesButton.tsx |
| Bans | DONE | | GuildBansTab.tsx |
| Delete community | DONE | sudo: password only | GuildDeleteModal.tsx |
| Transfer ownership | DONE | | TransferOwnershipModal.tsx |

### 1.3 Channel / thread / forum / group settings (ours guild/channel.rs)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Channel overview | PARTIAL | multi-line topic, voice bitrate/user limit/region, link URL | channel_tabs/ChannelOverviewTab.tsx |
| Channel permissions | PARTIAL | network member search, sync-with-category, thread rows | channel_tabs/ChannelPermissionsTab.tsx |
| Channel invites | DONE | | ChannelSettingsModalUtils.ts |
| Channel webhooks | DONE | | channel_tabs/ChannelWebhooksTab.tsx |
| Delete channel | DONE | | ChannelDeleteModal.tsx |
| Forum settings / tags | PARTIAL | tag add/edit/delete (ForumTagEditModal), default reaction picker, guidelines textarea, thread defaults, mature content | F/forum/components/settings/* |
| Thread settings | PARTIAL | slowmode; no live write test | F/threads/…/ThreadSettingsModal.tsx |
| Group DM edit | DONE | | modals/EditGroupModal.tsx |

## 2. Feature areas

### auth/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Instance picker, login, TOTP, IP auth, register, forgot pw, verify nagbar | DONE | | F/auth/* |
| Passkeys | PARTIAL | USB only | D/Passkeys.ts |
| Reset pw / verify email / recover pages | MISSING | web-only links | F/auth/… |
| SSO login | MISSING | | AuthSsoPanel.tsx |
| Browser login handoff | MISSING | | BrowserLoginHandoffModal.tsx |
| Multi-account switcher | DONE | | AccountSwitcherModal.tsx |
| Sudo verification | DONE | | SudoVerificationModal.tsx |
| Account restriction / age gate / new-device | MISSING | | AccountRestrictionScreen.tsx |
| OAuth2 authorize | MISSING | | OAuthAuthorizeModal.tsx |
| Terms acceptance | MISSING | | TermsAcceptanceModal.tsx |
| Token in OS keyring | DONE | falls back to the 0600 file when no keyring | D/DesktopAppStorage.ts |

### app / navigation / window / ui
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Guild rail, user area, UI kit, keyboard nav | DONE | | |
| Guild folders | PARTIAL | expand/collapse | GuildFolderItem.tsx |
| Rail drag-reorder / create folder | MISSING | | F/app/components/layout/dnd/* |
| Favorites | MISSING | | FavoritesButton.tsx |
| Set custom status | PARTIAL | emoji picker, live preview | CustomStatusModal.tsx |
| Nagbars (connection, notice, maintenance…) | PARTIAL | instance notice, maintenance, desktop-notification, premium ones (connection, update, streamer, verify done) | app_layout/nagbars/* |
| Skeletons | PARTIAL | member rows | F/app/components/skeleton/* |
| Reopen last location | DONE | | useLastLocationPersistence.ts |
| Window title unread count | DONE | | useFluxerDocumentTitle.ts |
| Toasts | DONE | only used by add-to-dictionary so far | Toasts.tsx |
| Pop-out windows | MISSING | | PopoutWindow.tsx |
| Error / rate-limit modals | PARTIAL | | GenericErrorModal.tsx |

### channel/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Channel list, header, message list, composer, member list, friends, notes, pins, uploads | DONE | | |
| Channel-list typing / unread on muted | MISSING | | InterfaceTab.tsx |
| Header bell dropdown | MISSING | | ChannelNotificationSettingsDropdown.tsx |
| Topic modal | DONE | | ChannelTopicModal.tsx |
| DM call buttons | MISSING | | CallButtons.tsx |
| Announcement follow | DONE | | ChannelFollowModal.tsx |
| Compact layout | MISSING | | CompactMessageLayout.tsx |
| Welcome sections | DONE | | ChannelWelcomeSection.tsx |
| Blocked message groups | DONE | | BlockedMessageGroups.tsx |
| Typing indicator tiers | PARTIAL | | ChannelTyping.tsx |
| Rich composer (pills, toolbar) | MISSING | | F/lexical/* |
| Emoji / GIF / sticker / memes pickers | PARTIAL | right-click Favorite/Copy menu; sticker sends at once (no pending sticker); GIF favorites import/export; saved-media upload/delete; premium upsell; frecency/favorites local, not synced via settings proto | pickers/*, EmojiPickerPopout.tsx |
| Reaction picker | DONE | | emoji_picker/* |
| Composer context menu | MISSING | | TextareaContextMenu.tsx |
| Slash commands | PARTIAL | /nick /kick /ban /tts /dm /gif | useCommands.ts |
| @everyone confirmation | DONE | | MentionEveryonePopout.tsx |
| Attachments | PARTIAL | video/audio play only where Rune's `Player` does (macOS); elsewhere "Open in browser" (pre-signed and multipart uploads live-verified) | embeds/attachments/* |
| Embeds | PARTIAL | gifv is a still + GIF badge; YouTube opens the browser (no iframe); text-only embeds don't shrink to fit | RichEmbed.tsx |
| Invite embed | PARTIAL | splash, guild badges, context menu, invites-paused state | InviteEmbed.tsx |
| NSFW gate / blur | PARTIAL | gate agreement lasts the session (Fluxer persists it); no geo/age-check variants | MatureContentChannelGate.tsx |
| Guild members page | MISSING | | GuildMembersPage.tsx |
| DM list | PARTIAL | pinned DMs, previews | DirectMessageList.tsx |
| Create DM / group DM | DONE | | CreateDMModal.tsx |
| Group DM management | PARTIAL | member add/remove only from the user menu | EditGroupModal.tsx |
| Active Now | MISSING | | ActiveNowSidebar.tsx |
| Private channel create step | PARTIAL | | ChannelCreateAccessStep.tsx |
| Duplicate channel | DONE | | ChannelDuplicateModal.tsx |
| Search UI | PARTIAL | jump highlight also tints the matching result (unchecked vs Fluxer); results and indexing state live-verified | message_search_bar/* |
| Call system message / system message wording | MISSING/PARTIAL | | CallMessage.tsx, SystemMessageUtils.tsx |

### messaging/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Send/edit/delete/reply/react/pin/unread/copy, delete confirm, error modals | DONE | (live-verified against a real instance, `--live=write`: nonce dedupe, edit, react, reply, pin/unpin, mark unread, delete) | |
| Offline queue | PARTIAL | | MessageQueue.tsx |
| Action bar / context menu | PARTIAL | shift layout | MessageContextMenu.tsx |
| Forward | DONE | | ForwardModal.tsx |
| Saved messages | MISSING | | SavedMessagesPage.tsx |
| Inbox / recent mentions | PARTIAL | no paging past 25; unread tab lists channels only | InboxPopout.tsx |
| Who reacted | PARTIAL | reactors past the first 100; filter right-click "Remove reaction" (it's in the message menu) | ReactionTooltip.tsx, MessageReactionsModal.tsx |
| Media viewer | DONE | | MediaModal.tsx |
| Edit alt text | MISSING | | EditAltTextModal.tsx |
| External link warning | DONE | | ExternalLinkWarningModal.tsx |
| Markdown alerts/tables | PARTIAL | | markdown/renderers/* |
| Code highlighting | DONE | | F/code_highlighting/* |
| Crosspost | DONE | | MessageCrosspostConfirmModal.tsx |

### notification / read_state / typing / presence
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Desktop notifications | PARTIAL | strip markdown; clicks on Win/mac | MessageNotificationPreview.ts |
| Sounds, status select, custom status display | DONE | | |
| Acks | PARTIAL | focus check (delayed auto-ack live-verified) | NotificationAutoAck.ts |
| Timed mute expiry | DONE | | useMuteSheet.ts |
| Per-channel overrides | MISSING | | GuildNotificationSettingsModal.tsx |
| Mute duration submenu | DONE | | MuteTargetMenuItem.tsx |
| TTS | MISSING | | TextToSpeech.tsx |
| Auto-idle | MISSING | | LocalPresence.ts |
| Thread/forum notification levels | PARTIAL | forum-wide unread (FORUM_UNREADS); no live test | F/threads/* |

### relationship / user
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Friends actions, notes | DONE | | |
| Friend nicknames | DONE | | ChangeFriendNicknameModal.tsx |
| Add friends to group | MISSING | | AddFriendsToGroupModal.tsx |
| Profile popout | DONE | | UserProfilePopout.tsx |
| Full profile modal | DONE | | UserProfileModal.tsx |
| Change own nickname | DONE | | ChangeNicknameModal.tsx |
| Community member avatars | PARTIAL | | F/user/components/profile/* |
| User context menu | PARTIAL | call, invite, local mute | UserContextMenu.tsx |

### guild
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Create/join, header popout, context menu, privacy modal, leave | DONE | | |
| Notification modal | PARTIAL | channel overrides | GuildNotificationSettingsModal.tsx |
| Edit community profile | DONE | | MyProfileTab.tsx |
| Delete my messages | MISSING | | useDeleteMyMessagesInGuild.tsx |
| Preview nagbar | MISSING | | GuildPreviewNagbar.tsx |
| Role icons | MISSING | | RoleIcon.tsx |
| Folder settings | MISSING | | GuildFolderSettingsModal.tsx |

### moderation
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Kick / Ban | DONE | | KickMemberModal.tsx, BanMemberModal.tsx |
| Timeout | DONE | | TimeoutMemberModal.tsx |
| Ban details | DONE | | BanDetailsModal.tsx |
| Report | PARTIAL | untested against a live server | ReportFlowModal.tsx |
| Voice moderation | MISSING | | VoiceParticipantMenuItems.tsx |

### voice
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Join/leave, panel, mute/deafen, rings, devices, HW encode | DONE | | |
| Screen share | PARTIAL | picker UI, quality, audio | ScreenSharePickerModal.tsx |
| Watch stream | PARTIAL | volume, fullscreen, spectators | WatchStreamOverlay.tsx |
| Camera | MISSING | | CameraPreviewModal.tsx |
| Call view grid | MISSING | | VoiceCallView.tsx |
| DM calls / ringing / P2P | MISSING | | IncomingCallUI.tsx |
| Per-user volume / local mute | MISSING | | VoiceParticipantMenuItems.tsx |
| Noise suppression | MISSING | | AudioProcessingModal.tsx |
| Voice details popout | MISSING | | VoiceDetailsPopout.tsx |
| PiP / pop-out | MISSING | | PiPOverlay.tsx |
| PTT / VAD | MISSING | | HoldSources.ts |
| Entrance sounds | MISSING | | EntranceSoundTrimmerModal.tsx |
| Moderator move / reconnect | PARTIAL | | VoiceReconnectionManager.tsx |
| E2EE indicator | MISSING | | VoiceE2EEIndicator.tsx |
| Stream audio (Linux) | MISSING | | AudioSourcePickerLinux.tsx |
| Join confirm / channel full | PARTIAL | | VoiceConnectionConfirmModal.tsx |
| Voice messages | MISSING | | VoiceMessageComposerModal.tsx |

### threads / forum
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Threads | PARTIAL | resizable panel, browser search, hoisted-role groups in thread members, timeouts; untested against a live server (demo fixtures + store tests only) | F/threads/* |
| Forum channels + tags | PARTIAL | server-side search/tag filter (client-side over loaded posts), media-channel gallery grid; untested against a live server | F/forum/* |

### expressions / emoji / gift / invite / discovery / search / input / theme / i18n / a11y / misc
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Animated emoji/avatars/stickers | DONE | | |
| Expression info popout, favorite memes, clone expression | PARTIAL | Join from the card; clone targets listed in the card, not a submenu; slot-full checks | F/expressions/* |
| Gifts *(low)* | MISSING | | F/gift/* |
| Invite: friends list, accept preview, pause | PARTIAL/MISSING | | F/invite/* |
| Discovery page | PARTIAL | card images, language filter | DiscoveryPage.tsx |
| Search UI chips/suggestions, history | PARTIAL/MISSING | | F/search/* |
| Quick switcher recents | PARTIAL | | QuickSwitcher.tsx |
| Custom keybinds, cheatsheet modal, keyboard mode | MISSING/PARTIAL | | F/input/* |
| Custom themes, Quick CSS, theme sharing | MISSING | | F/theme*/* |
| i18n coverage, plurals, RTL, locale dates | PARTIAL | plurals, RTL (locale dates + 12/24h done) | F/i18n/* |
| Reduced motion, screen reader announcements | MISSING/PARTIAL | | F/accessibility/* |
| Streamer mode | PARTIAL | names masked via Store::display_name, account gate, invite link, sounds, notifications; notes in profile not hidden | F/streamer_mode/* |
| Debug modals, experiments, account engine cache, premium *(low)* | MISSING | | |

## 3. Desktop shell
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Tray, close to tray, flash, open links, HW encoder | DONE | | |
| Minimize to tray / start minimized | PARTIAL | WONTFIX (user decision): minimize to tray — raw-window-handle workaround works only on X11/Windows/macOS, not Wayland | D/Window.ts |
| Notifications | PARTIAL | click routing Win/mac | D/NotificationsIpc.ts |
| Badge | PARTIAL | Win/mac | D/UnreadBadge.ts |
| Deep links | PARTIAL | Windows forwarding | D/DeepLinks.ts |
| Autostart | DONE | Flatpak Background portal; Win/mac compile-checked only | D/Autostart.ts |
| Window state persistence | PARTIAL | WONTFIX (user decision): size only — position/maximized need the raw handle and are impossible on Wayland | D/Window.ts |
| Updater | PARTIAL | no release feed yet (`REFLUX_UPDATE_FEED`); AppImage self-update, else opens the page | D/Updater.ts |
| Global shortcuts / PTT | PARTIAL | Linux portal only (no X11 fallback), Windows mute/deafen only, macOS none; PTT not used by voice yet (`shortcuts::ptt_held`) | D/GlobalShortcutsEngine.ts |
| Spellcheck | DONE | OS engine, language auto-detect; menu targets the caret's word | D/Spellcheck.ts |
| Zoom | DONE | | D/Menu.ts |
| Screen-share picker / app audio | PARTIAL | | D/DisplayMedia.ts |
| App menu (macOS) | MISSING | | D/Menu.ts |
| File downloads | DONE | | D/FileDownloads.ts |
| Copy image | DONE | | D/MediaClipboard.ts |
| Streamer-mode detection, theme folder, troubleshooting *(low)* | PARTIAL | theme folder, troubleshooting (OBS/XSplit detection done) | |
