//! `RoleIcon`: a role's uploaded icon or unicode emoji beside a name, from the member's highest
//! display role that has one (`GuildMember.getIconRole`).

use reflux_model::{Role, Snowflake};
use reflux_store::Store;
use rune::{El, el};

use super::{ShellState, emoji};
use crate::media::Load;

/// The member's highest role with an icon or emoji.
pub fn icon_role(store: &Store, guild: Snowflake, user: Snowflake) -> Option<&Role> {
    let member = store.member(guild, user)?;
    let roles = &store.guilds.get(&guild)?.roles;
    roles
        .iter()
        .filter(|r| member.roles.contains(&r.id) && (r.icon.is_some() || r.unicode_emoji.is_some()))
        .max_by_key(|r| (r.position, std::cmp::Reverse(r.id)))
}

/// `getRoleIconURL`: `role-icons`, sized 16–512.
fn url(base: &str, role: Snowflake, hash: &str, css: f32, scale: f32) -> String {
    let size = crate::media::snap(css * scale.max(2.0)).clamp(16, 512);
    format!("{}/role-icons/{role}/{hash}.webp?size={size}", base.trim_end_matches('/'))
}

/// The emoji's sprite cell: a glyph, or a name with or without colons.
fn sprite(raw: &str) -> Option<usize> {
    emoji::sprite_index(raw).or_else(|| {
        let name = raw.trim_matches(':');
        emoji::all().iter().flat_map(|(_, v)| v).find(|e| e.name == name).and_then(|e| emoji::sprite_index(&e.glyph))
    })
}

/// A `size` box (16, 20 or 40) with the icon, or the emoji at 14/18/36; `None` until it loads
/// or when it can't (`onError` hides it).
pub fn view(st: &ShellState, role: &Role, size: f32) -> Option<El> {
    let (load, side) = match (&role.icon, &role.unicode_emoji) {
        (Some(h), _) => (st.media.asset(|b, s| url(b, role.id, h, size, s)), size),
        (None, Some(e)) => (sprite(e).map_or(Load::Failed, |i| st.media.sprite(i)), if size >= 40.0 { size - 4.0 } else { size - 2.0 }),
        _ => return None,
    };
    let Load::Ready(px) = load else { return None };
    Some(row_box(size).child(el().size(side, side).pixels_fit(&px, rune::SurfaceFit::Fit)).label(format!("Role icon, {}", role.name)))
}

fn row_box(size: f32) -> El {
    rune::row().size(size, size).center()
}

#[cfg(test)]
mod tests {
    #[test]
    fn role_icon_urls_snap_to_the_proxy_ladder() {
        let u = super::url("https://m.example/", reflux_model::Snowflake(7), "abc", 20.0, 1.0);
        assert!(u.starts_with("https://m.example/role-icons/7/abc.webp?size="), "{u}");
    }
}
