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
| Appearance | MISSING | cozy/compact, spacing, hide avatars, font size, typing in channel list, unread on muted, streamer mode | tabs/AppearanceTab.tsx |
| Themes | PARTIAL | legacy dark, system, custom colours/fonts, Quick CSS, sharing | F/theme_settings/* |
| Notifications | PARTIAL | unread badge toggle, per-sound toggles, custom sounds, TTS | tabs/NotificationsTab.tsx |
| Chat (Messages & media) | MISSING | embeds/link previews/reactions/spoilers display, input behaviour | tabs/ChatSettingsTab.tsx |
| Voice & video | PARTIAL | volumes, mic test, processing, PTT/VAD, entrance sound, camera, stream quality | tabs/UserVoiceTab.tsx, UserVideoTab.tsx |
| Accessibility | MISSING | reduced motion, animated emoji, saturation, link underline, keyboard mode | tabs/AccessibilityTab.tsx |
| Language & time | PARTIAL | 12/24h, auto-detect, spellcheck | tabs/LanguageTab.tsx |
| Keybinds | PARTIAL | custom keybinds, recorder, PTT key, global shortcuts | tabs/KeybindsTab.tsx |
| Desktop app | PARTIAL | autostart, start minimized, window state, minimize to tray | tabs/DesktopSettingsTab.tsx |
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
| Moderation | MISSING | | guild_tabs/GuildModerationTab.tsx |
| Activity log | PARTIAL | filters, paging, change details | guild_tabs/GuildAuditLogTab.tsx |
| Emoji | PARTIAL | rename, bulk upload | guild_tabs/GuildEmojiTab.tsx |
| Stickers | MISSING | | guild_tabs/GuildStickersTab.tsx |
| Explore (discovery listing) | MISSING | | guild_tabs/GuildDiscoveryTab.tsx |
| Vanity URL | MISSING | | guild_tabs/GuildVanityURLTab.tsx |
| Webhooks | MISSING | | guild_tabs/GuildWebhooksTab.tsx, F/webhook/* |
| Members | PARTIAL | timeout, nickname, filters, paging | GuildMembersPage.tsx |
| Invites | PARTIAL | pause invites | DisableInvitesButton.tsx |
| Bans | DONE | ban details modal | GuildBansTab.tsx |
| Delete community | DONE | sudo: password only | GuildDeleteModal.tsx |
| Transfer ownership | MISSING | | TransferOwnershipModal.tsx |

### 1.3 Channel / thread / forum / group settings (ours guild/channel.rs)
| Tab | Status | Missing | Official source |
|---|---|---|---|
| Channel overview | PARTIAL | multi-line topic, voice bitrate/user limit/region, link URL | channel_tabs/ChannelOverviewTab.tsx |
| Channel permissions | PARTIAL | network member search, sync-with-category, thread rows | channel_tabs/ChannelPermissionsTab.tsx |
| Channel invites | MISSING | | ChannelSettingsModalUtils.ts |
| Channel webhooks | MISSING | | channel_tabs/ChannelWebhooksTab.tsx |
| Delete channel | DONE | | ChannelDeleteModal.tsx |
| Forum settings / tags | MISSING | | F/forum/components/settings/* |
| Thread settings | MISSING | | F/threads/…/ThreadSettingsModal.tsx |
| Group DM edit | MISSING | | modals/EditGroupModal.tsx |

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
| Nagbars (connection, notice, maintenance…) | MISSING | | app_layout/nagbars/* |
| Skeletons | PARTIAL | member rows | F/app/components/skeleton/* |
| Reopen last location | MISSING | | useLastLocationPersistence.ts |
| Window title unread count | MISSING | | useFluxerDocumentTitle.ts |
| Toasts | MISSING | | Toasts.tsx |
| Pop-out windows | MISSING | | PopoutWindow.tsx |
| Error / rate-limit modals | PARTIAL | | GenericErrorModal.tsx |

### channel/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Channel list, header, message list, composer, member list, friends, notes, pins, uploads | DONE | | |
| Channel-list typing / unread on muted | MISSING | | InterfaceTab.tsx |
| Header bell dropdown | MISSING | | ChannelNotificationSettingsDropdown.tsx |
| Topic modal | MISSING | | ChannelTopicModal.tsx |
| DM call buttons | MISSING | | CallButtons.tsx |
| Announcement follow | MISSING | | ChannelFollowModal.tsx |
| Compact layout | MISSING | | CompactMessageLayout.tsx |
| Welcome sections | MISSING | | ChannelWelcomeSection.tsx |
| Blocked message groups | MISSING | | BlockedMessageGroups.tsx |
| Typing indicator tiers | PARTIAL | | ChannelTyping.tsx |
| Rich composer (pills, toolbar) | MISSING | | F/lexical/* |
| Emoji / GIF / sticker / memes pickers | MISSING | buttons inert | pickers/*, EmojiPickerPopout.tsx |
| Reaction picker | PARTIAL | custom, recents, skin tones, tabs | emoji_picker/* |
| Composer context menu | MISSING | | TextareaContextMenu.tsx |
| Slash commands | PARTIAL | /nick /kick /ban /tts /dm /gif | useCommands.ts |
| @everyone confirmation | MISSING | | MentionEveryonePopout.tsx |
| Attachments | PARTIAL | video/audio play only where Rune's `Player` does (macOS); elsewhere "Open in browser" (pre-signed and multipart uploads live-verified) | embeds/attachments/* |
| Embeds | PARTIAL | gifv is a still + GIF badge; YouTube opens the browser (no iframe); text-only embeds don't shrink to fit | RichEmbed.tsx |
| Invite embed | PARTIAL | splash, guild badges, context menu, invites-paused state | InviteEmbed.tsx |
| NSFW gate / blur | PARTIAL | gate agreement lasts the session (Fluxer persists it); no geo/age-check variants | MatureContentChannelGate.tsx |
| Guild members page | MISSING | | GuildMembersPage.tsx |
| DM list | PARTIAL | pinned DMs, previews | DirectMessageList.tsx |
| Create DM / group DM | MISSING | | CreateDMModal.tsx |
| Group DM management | MISSING | | EditGroupModal.tsx |
| Active Now | MISSING | | ActiveNowSidebar.tsx |
| Private channel create step | PARTIAL | | ChannelCreateAccessStep.tsx |
| Duplicate channel | MISSING | | ChannelDuplicateModal.tsx |
| Search UI | PARTIAL | jump highlight also tints the matching result (unchecked vs Fluxer); results and indexing state live-verified | message_search_bar/* |
| Call system message / system message wording | MISSING/PARTIAL | | CallMessage.tsx, SystemMessageUtils.tsx |

### messaging/
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Send/edit/delete/reply/react/pin/unread/copy, delete confirm, error modals | DONE | (live-verified against a real instance, `--live=write`: nonce dedupe, edit, react, reply, pin/unpin, mark unread, delete) | |
| Offline queue | PARTIAL | | MessageQueue.tsx |
| Action bar / context menu | PARTIAL | shift layout; forward, thread, bookmark, suppress embeds, remove reactions, report | MessageContextMenu.tsx |
| Forward | MISSING | | ForwardModal.tsx |
| Saved messages | MISSING | | SavedMessagesPage.tsx |
| Inbox / recent mentions | MISSING | | InboxPopout.tsx |
| Who reacted | MISSING | | ReactionTooltip.tsx, MessageReactionsModal.tsx |
| Media viewer | DONE | | MediaModal.tsx |
| Edit alt text | MISSING | | EditAltTextModal.tsx |
| External link warning | DONE | | ExternalLinkWarningModal.tsx |
| Markdown alerts/tables | PARTIAL | | markdown/renderers/* |
| Code highlighting | DONE | | F/code_highlighting/* |
| Crosspost | MISSING | | MessageCrosspostConfirmModal.tsx |

### notification / read_state / typing / presence
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Desktop notifications | PARTIAL | strip markdown; clicks on Win/mac | MessageNotificationPreview.ts |
| Sounds, status select, custom status display | DONE | | |
| Acks | PARTIAL | focus check (delayed auto-ack live-verified) | NotificationAutoAck.ts |
| Timed mute expiry | PARTIAL | | useMuteSheet.ts |
| Per-channel overrides | MISSING | | GuildNotificationSettingsModal.tsx |
| Mute duration submenu | PARTIAL | | MuteTargetMenuItem.tsx |
| TTS | MISSING | | TextToSpeech.tsx |
| Auto-idle | MISSING | | LocalPresence.ts |
| Thread/forum notification levels | MISSING | | F/threads/* |

### relationship / user
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Friends actions, notes | DONE | | |
| Friend nicknames | MISSING | | ChangeFriendNicknameModal.tsx |
| Add friends to group | MISSING | | AddFriendsToGroupModal.tsx |
| Profile popout | DONE | | UserProfilePopout.tsx |
| Full profile modal | DONE | | UserProfileModal.tsx |
| Change own nickname | DONE | | ChangeNicknameModal.tsx |
| Community member avatars | PARTIAL | | F/user/components/profile/* |
| User context menu | PARTIAL | call, invite, friend nickname, local mute, moderation | UserContextMenu.tsx |

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
| Kick / Ban | PARTIAL | from user menu, reason, delete history | KickMemberModal.tsx, BanMemberModal.tsx |
| Timeout | MISSING | | TimeoutMemberModal.tsx |
| Ban details | MISSING | | BanDetailsModal.tsx |
| Report | MISSING | | ReportFlowModal.tsx |
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
| Threads | MISSING | | F/threads/* |
| Forum channels + tags | MISSING | | F/forum/* |

### expressions / emoji / gift / invite / discovery / search / input / theme / i18n / a11y / misc
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Animated emoji/avatars/stickers | DONE | | |
| Expression info popout, favorite memes, clone expression | MISSING | | F/expressions/* |
| Gifts *(low)* | MISSING | | F/gift/* |
| Invite: friends list, accept preview, pause | PARTIAL/MISSING | | F/invite/* |
| Discovery page | MISSING | | DiscoveryPage.tsx |
| Search UI chips/suggestions, history | PARTIAL/MISSING | | F/search/* |
| Quick switcher recents | PARTIAL | | QuickSwitcher.tsx |
| Custom keybinds, cheatsheet modal, keyboard mode | MISSING/PARTIAL | | F/input/* |
| Custom themes, Quick CSS, theme sharing | MISSING | | F/theme*/* |
| i18n coverage, plurals, RTL, locale dates | PARTIAL/MISSING | | F/i18n/* |
| Reduced motion, screen reader announcements | MISSING/PARTIAL | | F/accessibility/* |
| Streamer mode | MISSING | | F/streamer_mode/* |
| Debug modals, experiments, account engine cache, premium *(low)* | MISSING | | |

## 3. Desktop shell
| Feature | Status | Missing | Official source |
|---|---|---|---|
| Tray, close to tray, flash, open links, HW encoder | DONE | | |
| Minimize to tray / start minimized | MISSING | | D/Window.ts |
| Notifications | PARTIAL | click routing Win/mac | D/NotificationsIpc.ts |
| Badge | PARTIAL | Win/mac | D/UnreadBadge.ts |
| Deep links | PARTIAL | Windows forwarding | D/DeepLinks.ts |
| Autostart | MISSING | | D/Autostart.ts |
| Window state persistence | MISSING | | D/Window.ts |
| Updater | MISSING | | D/Updater.ts |
| Global shortcuts / PTT | MISSING | | D/GlobalShortcutsEngine.ts |
| Spellcheck | MISSING | | D/Spellcheck.ts |
| Zoom | MISSING | | D/Menu.ts |
| Screen-share picker / app audio | PARTIAL | | D/DisplayMedia.ts |
| App menu (macOS) | MISSING | | D/Menu.ts |
| File downloads | DONE | | D/FileDownloads.ts |
| Copy image | DONE | | D/MediaClipboard.ts |
| Streamer-mode detection, theme folder, troubleshooting *(low)* | MISSING | | |
