// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fluxer message markdown parser, vendored from `fluxer/packages/markdown_parser/rust`
//! without its wasm/native/JSON/binary FFI layers.

pub mod ast;
mod block;
pub mod constants;
pub mod emoji;
mod inline;
mod links;
mod normalize;
mod parser;
pub mod plaintext;
mod scan;
mod text;

pub use ast::{
    AlertType, EmojiKind, GuildNavigationType, ListItem, MentionKind, Node, ParserFlags,
    TableAlignment, TimestampStyle,
};
pub use emoji::EmojiContext;
pub use parser::{MarkdownParser, ParseError};
pub use plaintext::{PlaintextOptions, render_ast_to_plaintext};

/// Parser options: `flags` is a bitset of [`ParserFlags`]; `emoji` holds the
/// caller-detected unicode emoji / shortcode table (see [`EmojiContext::parse`]).
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub flags: u32,
    /// Shared so a parse doesn't copy it.
    pub emoji: std::sync::Arc<EmojiContext>,
}

impl Options {
    pub fn new(flags: u32) -> Self {
        Self {
            flags,
            emoji: Default::default(),
        }
    }
}

/// Flag presets matching `fluxer_app/.../MarkdownParserFlags.ts`.
pub mod presets {
    use crate::ParserFlags as F;

    pub const STANDARD: u32 = F::ALL;
    pub const RESTRICTED_INLINE_REPLY: u32 = STANDARD
        & !(F::ALLOW_BLOCKQUOTES
            | F::ALLOW_MULTILINE_BLOCKQUOTES
            | F::ALLOW_TABLES
            | F::ALLOW_ALERTS
            | F::ALLOW_HEADINGS
            | F::ALLOW_LISTS);
    pub const RESTRICTED_INLINE_PREVIEW: u32 =
        RESTRICTED_INLINE_REPLY | F::ALLOW_HEADINGS | F::ALLOW_LISTS;
    pub const RESTRICTED_USER_BIO: u32 = STANDARD
        & !(F::ALLOW_HEADINGS
            | F::ALLOW_CODE_BLOCKS
            | F::ALLOW_ROLE_MENTIONS
            | F::ALLOW_EVERYONE_MENTIONS
            | F::ALLOW_SUBTEXT
            | F::ALLOW_TABLES
            | F::ALLOW_ALERTS);
    pub const RESTRICTED_EMBED_DESCRIPTION: u32 = STANDARD & !F::ALLOW_TABLES;
}

pub fn parse(input: &str, opts: &Options) -> Vec<Node> {
    let mut parser = MarkdownParser::new(opts.flags, opts.emoji.clone());
    match parser.parse(input) {
        Ok(nodes) => nodes,
        Err(never) => match never {},
    }
}
