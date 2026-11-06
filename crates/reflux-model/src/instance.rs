//! Instance discovery: `GET /.well-known/fluxer` (`http-api/instance.mdx`).

use std::collections::HashMap;

use crate::{Snowflake, Timestamp, auth::SsoStatus};

model! {
    pub struct InstanceDiscovery {
        pub codename: Option<String>,
        pub api_code_version: u32,
        pub endpoints: InstanceEndpoints,
        pub captcha: CaptchaConfig,
        pub features: InstanceFeatures,
        pub gif: GifProvider,
        pub sso: SsoStatus,
        pub registration: RegistrationPolicy,
        pub community: CommunityPolicy,
        pub services: ServiceAvailability,
        pub limits: LimitConfig,
        pub push: PushConfig,
        pub app_public: AppPublic,
        pub age_policy: AgePolicy,
        pub notice: Option<InstanceNotice>,
        pub guild_badges: Vec<GuildBadge>,
        pub guild_badges_partial: bool,
        pub custom_badges_version: Option<String>,
        pub built_in_guild_badge_images: Option<BuiltInGuildBadgeImages>,
        pub hidden_built_in_guild_badges: Vec<String>,
        pub voice_origins: Vec<String>,
    }

    pub struct InstanceEndpoints {
        pub api: String,
        pub api_client: String,
        pub api_public: String,
        pub api_transport: Option<String>,
        pub gateway: String,
        pub media: String,
        pub upload_relay: Option<String>,
        pub static_cdn: String,
        pub marketing: String,
        pub admin: String,
        pub invite: String,
        pub gift: String,
        pub webapp: String,
        pub docs: Option<String>,
    }

    pub struct CaptchaConfig {
        /// `altcha` or `none`.
        pub provider: String,
    }

    pub struct InstanceFeatures {
        pub voice_enabled: bool,
        pub stripe_enabled: bool,
        pub premium_enabled: bool,
        pub stripe_serviceable: bool,
        pub self_hosted: bool,
        pub presigned_attachment_uploads: bool,
        pub emails_enabled: bool,
        /// Missing means true.
        pub discovery_enabled: Option<bool>,
        /// `email` or `username`.
        pub account_identity: String,
        /// `none` or `random`.
        pub tag_style: String,
        pub desktop_modules_enabled: bool,
        pub account_deletion_grace_period_hours: Option<u32>,
        pub identifier_first: bool,
        pub instant_accounts: bool,
        pub email_codes: bool,
        pub date_of_birth_step: bool,
        pub instant_account_availability: Option<String>,
        pub interaction_tier: Option<String>,
        pub guild_join_gate: Option<String>,
        pub email_delivery_state: Option<String>,
        pub verification_basis: Option<String>,
    }

    pub struct GifProvider {
        pub provider: String,
        pub display_name: String,
        pub attribution_required: bool,
    }

    pub struct RegistrationPolicy {
        /// `open`, `approval` or `closed`.
        pub mode: String,
        pub admin_registration_urls_enabled: bool,
    }

    pub struct CommunityPolicy {
        pub single_community: bool,
        pub single_community_guild_id: Option<Snowflake>,
        pub direct_messages_disabled: bool,
        pub friends_disabled: bool,
        /// Missing means enabled.
        pub personal_notes_enabled: Option<bool>,
        pub guild_create_access: bool,
    }

    pub struct ServiceAvailability {
        pub gif_enabled: bool,
        pub youtube_enabled: bool,
        pub bluesky_enabled: bool,
    }

    pub struct LimitConfig {
        pub version: u32,
        #[serde(rename = "traitDefinitions")]
        pub trait_definitions: Vec<String>,
        pub rules: Vec<LimitRule>,
        #[serde(rename = "defaultsHash")]
        pub defaults_hash: String,
    }

    pub struct LimitRule {
        pub id: String,
        pub filters: Option<LimitFilters>,
        pub overrides: HashMap<String, i64>,
    }

    pub struct LimitFilters {
        pub traits: Vec<String>,
        #[serde(rename = "guildFeatures")]
        pub guild_features: Vec<String>,
    }

    pub struct PushConfig {
        pub public_vapid_key: Option<String>,
        pub delivery_mode: Option<String>,
        pub event_resolution_path: Option<String>,
        pub external_relay_url: Option<String>,
        pub relay_signing_key_hashes: Option<Vec<String>>,
    }

    pub struct AppPublic {
        pub branding: Branding,
        pub setup: SetupState,
        pub legal: LegalConfig,
        pub registration: RegistrationFields,
    }

    pub struct Branding {
        pub product_name: String,
        pub icon_url: Option<String>,
        pub symbol_url: Option<String>,
        pub logo_url: Option<String>,
        pub wordmark_url: Option<String>,
        pub favicon_url: Option<String>,
        pub favicon_dark_url: Option<String>,
        pub theme_color: Option<String>,
        pub status_page_url: Option<String>,
        pub status_page_instatus: bool,
        pub status_page_incident_history_url: Option<String>,
        pub premium_product_name: String,
        pub premium_info_url: Option<String>,
        pub sign_in_backdrop_url: Option<String>,
        pub premium_page_color: Option<String>,
        pub premium_showcase_expressions_url: Option<String>,
        pub premium_showcase_profile_url: Option<String>,
        pub premium_showcase_stream_url: Option<String>,
        pub premium_showcase_upload_url: Option<String>,
        pub custom_css: Option<String>,
        pub upload_limit_message: Option<String>,
        pub trusted_domains: Vec<String>,
    }

    pub struct SetupState {
        pub configured: bool,
        pub admin_url: Option<String>,
        pub account_identity_locked: bool,
        pub account_identity_lock_reason: Option<String>,
        pub setup_token_required: bool,
    }

    pub struct LegalConfig {
        pub terms_url: Option<String>,
        pub privacy_url: Option<String>,
        pub guidelines_url: Option<String>,
    }

    pub struct RegistrationFields {
        pub collect_date_of_birth: bool,
    }

    pub struct AgePolicy {
        pub geos: Vec<AgePolicyRegion>,
    }

    pub struct AgePolicyRegion {
        pub country_code: String,
        pub region_code: Option<String>,
        /// `restrict` or `block`.
        pub action: String,
    }

    pub struct InstanceNotice {
        pub id: String,
        pub message: String,
        pub url: Option<String>,
        pub starts_at: Option<Timestamp>,
        pub ends_at: Option<Timestamp>,
    }

    pub struct GuildBadge {
        pub id: Snowflake,
        pub name: String,
        pub description: Option<String>,
        pub asset_url: String,
        pub href: Option<String>,
        pub position: Option<String>,
    }

    pub struct BuiltInGuildBadgeImages {
        pub verified: Option<String>,
        pub partnered: Option<String>,
        pub discoverable: Option<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_discovery_fixture() {
        // From packages/instance_bootstrap/src/__tests__/DiscoveryFixtures.ts.
        let d: InstanceDiscovery = serde_json::from_str(
            r##"{"api_code_version":1,"endpoints":{"api":"https://api.fluxer.app",
            "api_client":"https://api.fluxer.app","api_public":"https://api.fluxer.app",
            "gateway":"wss://gateway.fluxer.app","media":"https://media.fluxer.app",
            "static_cdn":"https://cdn.fluxer.app","marketing":"https://fluxer.app",
            "admin":"https://admin.fluxer.app","invite":"https://fluxer.gg","gift":"https://fluxer.gift",
            "webapp":"https://web.fluxer.app"},"captcha":{"provider":"altcha"},
            "features":{"voice_enabled":true,"stripe_enabled":true,"self_hosted":false,
            "presigned_attachment_uploads":true,"emails_enabled":true},
            "gif":{"provider":"klipy","display_name":"Klipy","attribution_required":false},
            "sso":{"enabled":false,"enforced":false,"display_name":null,"button_label":null,
            "redirect_uri":"https://web.fluxer.app/sso/callback"},
            "registration":{"mode":"open","admin_registration_urls_enabled":true},
            "community":{"single_community":false,"single_community_guild_id":null,
            "direct_messages_disabled":false},
            "services":{"gif_enabled":true,"youtube_enabled":true,"bluesky_enabled":false},
            "limits":{"version":2,"traitDefinitions":["premium"],"rules":[
            {"id":"default","overrides":{"max_guild_emojis":50}},
            {"id":"premium","filters":{"traits":["premium"],"guildFeatures":[]},
            "overrides":{"max_guild_emojis":250}}],"defaultsHash":"deployed-defaults-hash"},
            "push":{"public_vapid_key":"BPublicVapidKey"},
            "app_public":{"branding":{"product_name":"Fluxer","icon_url":"https://cdn.fluxer.app/icon.png",
            "theme_color":"#5865f2"},"setup":{"configured":true,"admin_url":"https://admin.fluxer.app"},
            "legal":{"terms_url":"https://fluxer.app/terms","privacy_url":"https://fluxer.app/privacy"},
            "registration":{"collect_date_of_birth":true}}}"##,
        )
        .unwrap();
        assert_eq!(d.endpoints.gateway, "wss://gateway.fluxer.app");
        assert_eq!(d.captcha.provider, "altcha");
        assert_eq!(d.limits.rules[1].filters.as_ref().unwrap().traits, ["premium"]);
        assert_eq!(d.app_public.branding.product_name, "Fluxer");
    }
}
