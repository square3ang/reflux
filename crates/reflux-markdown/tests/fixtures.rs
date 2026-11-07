// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported from fluxer/packages/markdown_parser/rust/tests/{fixtures,binary_envelope}.rs.

use reflux_markdown::{EmojiContext, Node, Options, ParserFlags, presets};
use serde_json::{Value, json};

fn nodes(input: &str, flags: u32, emoji_context: &str) -> Vec<Node> {
    reflux_markdown::parse(
        input,
        &Options {
            flags,
            emoji: EmojiContext::parse(emoji_context).into(),
        },
    )
}

fn parse(input: &str, flags: u32, emoji_context: &str) -> Value {
    json!({"nodes": nodes(input, flags, emoji_context)})
}

#[test]
fn native_parser_fixtures_cover_typescript_suite_surface() {
    let inline_flags = ParserFlags::ALLOW_SPOILERS
        | ParserFlags::ALLOW_MASKED_LINKS
        | ParserFlags::ALLOW_AUTOLINKS
        | ParserFlags::ALLOW_USER_MENTIONS
        | ParserFlags::ALLOW_CHANNEL_MENTIONS
        | ParserFlags::ALLOW_EVERYONE_MENTIONS;
    assert_eq!(parse("", 0, ""), json!({"nodes": []}));
    assert_eq!(
        parse(
            "# Heading\n||secret||",
            ParserFlags::ALLOW_HEADINGS | ParserFlags::ALLOW_SPOILERS,
            ""
        ),
        json!({"nodes":[
            {"type":"Heading","level":1,"children":[{"type":"Text","content":"Heading"}]},
            {"type":"Spoiler","children":[{"type":"Text","content":"secret"}],"isBlock":false}
        ]})
    );
    assert_eq!(
        parse("**bold** *em* __under__ ~~strike~~ `code`", 0, ""),
        json!({"nodes":[
            {"type":"Strong","children":[{"type":"Text","content":"bold"}]},
            {"type":"Text","content":" "},
            {"type":"Emphasis","children":[{"type":"Text","content":"em"}]},
            {"type":"Text","content":" "},
            {"type":"Underline","children":[{"type":"Text","content":"under"}]},
            {"type":"Text","content":" "},
            {"type":"Strikethrough","children":[{"type":"Text","content":"strike"}]},
            {"type":"Text","content":" "},
            {"type":"InlineCode","content":"code"}
        ]})
    );
    assert_eq!(
        parse(
            "Nevermind ! It is on C:\\, i thought i moved it",
            ParserFlags::ALL,
            ""
        ),
        json!({"nodes":[
            {"type":"Text","content":"Nevermind ! It is on C:\\, i thought i moved it"}
        ]})
    );
    assert_eq!(
        parse(
            "C:\\. C:\\! C:\\? C:\\; C:\\/ C:\\' C:\\\" C:\\{ C:\\}",
            ParserFlags::ALL,
            ""
        ),
        json!({"nodes":[
            {"type":"Text","content":"C:\\. C:\\! C:\\? C:\\; C:\\/ C:\\' C:\\\" C:\\{ C:\\}"}
        ]})
    );
    assert_eq!(
        parse("1\\. not a list", ParserFlags::ALLOW_LISTS, ""),
        json!({"nodes":[
            {"type":"Text","content":"1. not a list"}
        ]})
    );
    assert_eq!(
        parse("@everyone <@123> <#456> <t:1234567890:R>", inline_flags, ""),
        json!({"nodes":[
            {"type":"Mention","kind":{"kind":"Everyone"}},
            {"type":"Text","content":" "},
            {"type":"Mention","kind":{"kind":"User","id":"123"}},
            {"type":"Text","content":" "},
            {"type":"Mention","kind":{"kind":"Channel","id":"456"}},
            {"type":"Text","content":" "},
            {"type":"Timestamp","timestamp":1234567890,"style":"RelativeTime"}
        ]})
    );
    assert_eq!(
        parse(
            "hello 😄 :smile:",
            0,
            "S\t6\t4\t😄\tsmile\t1f604\nC\tsmile\t😄\t1f604\n"
        ),
        json!({"nodes":[
            {"type":"Text","content":"hello "},
            {"type":"Emoji","kind":{"kind":"Standard","raw":"😄","codepoints":"1f604","name":"smile"}},
            {"type":"Text","content":" "},
            {"type":"Emoji","kind":{"kind":"Standard","raw":"😄","codepoints":"1f604","name":"smile"}}
        ]})
    );
    assert_eq!(
        parse("Sweden 🇸🇪", 0, ""),
        json!({"nodes":[
            {"type":"Text","content":"Sweden "},
            {"type":"Emoji","kind":{"kind":"Standard","raw":"🇸🇪","codepoints":"1f1f8-1f1ea","name":"flag_se"}}
        ]})
    );
    assert_eq!(
        parse("# \u{200e} \n# \u{200e}", ParserFlags::ALLOW_HEADINGS, ""),
        json!({"nodes":[
            {"type":"Heading","level":1,"children":[{"type":"Text","content":"\u{200e} "}]},
            {"type":"Heading","level":1,"children":[{"type":"Text","content":"\u{200e}"}]}
        ]})
    );
    assert_eq!(
        parse("> Sweden 🇸🇪", ParserFlags::ALLOW_BLOCKQUOTES, ""),
        json!({"nodes":[{
            "type":"Blockquote",
            "children":[
                {"type":"Text","content":"Sweden "},
                {"type":"Emoji","kind":{"kind":"Standard","raw":"🇸🇪","codepoints":"1f1f8-1f1ea","name":"flag_se"}}
            ]
        }]})
    );
    assert_eq!(
        parse("> ", ParserFlags::ALLOW_BLOCKQUOTES, ""),
        json!({"nodes":[{
            "type":"Blockquote",
            "children":[],
            "blankLines":1
        }]})
    );
    assert_eq!(
        parse("> \n>  \nsome text", ParserFlags::ALLOW_BLOCKQUOTES, ""),
        json!({"nodes":[
            {
                "type":"Blockquote",
                "children":[],
                "blankLines":2
            },
            {"type":"Text","content":"some text"}
        ]})
    );
    assert_eq!(
        parse("> \n>", ParserFlags::ALLOW_BLOCKQUOTES, ""),
        json!({"nodes":[
            {
                "type":"Blockquote",
                "children":[],
                "blankLines":1
            },
            {"type":"Text","content":">"}
        ]})
    );
}

// binary_envelope.rs golden bytes, expressed as the AST they encode.
#[test]
fn binary_envelope_goldens_as_ast() {
    let all = ParserFlags::ALL;
    assert_eq!(parse("", all, ""), json!({"nodes": []}));
    assert_eq!(
        parse("**b**", all, ""),
        json!({"nodes":[{"type":"Strong","children":[{"type":"Text","content":"b"}]}]})
    );
    assert_eq!(
        parse("<t:1234567890:R>", all, ""),
        json!({"nodes":[{"type":"Timestamp","timestamp":1234567890,"style":"RelativeTime"}]})
    );
    assert_eq!(
        parse("4. a", all, ""),
        json!({"nodes":[{"type":"List","ordered":true,"items":[
            {"children":[{"type":"Text","content":"a"}],"ordinal":4}
        ]}]})
    );
    assert_eq!(
        parse("[t](https://e.com/a)", all, ""),
        json!({"nodes":[{"type":"Link","text":{"type":"Text","content":"t"},
            "url":"https://e.com/a","escaped":false,"rawUrl":"https://e.com/a",
            "source":"[t](https://e.com/a)"}]})
    );
    assert_eq!(
        parse("# h\n||s||", all, ""),
        json!({"nodes":[
            {"type":"Heading","level":1,"children":[{"type":"Text","content":"h"}]},
            {"type":"Spoiler","children":[{"type":"Text","content":"s"}],"isBlock":false}
        ]})
    );
}

#[test]
fn presets_match_app_contexts() {
    assert_eq!(presets::STANDARD, ParserFlags::ALL);
    // Replies drop block syntax: a heading stays literal text.
    assert_eq!(
        parse("# h", presets::RESTRICTED_INLINE_REPLY, ""),
        json!({"nodes":[{"type":"Text","content":"# h"}]})
    );
    assert_eq!(
        parse("```rs\nfn x() {}\n```", presets::STANDARD, ""),
        json!({"nodes":[{"type":"CodeBlock","language":"rs","content":"fn x() {}\n"}]})
    );
}

#[test]
fn mentions_and_custom_emoji() {
    let all = ParserFlags::ALL;
    assert_eq!(
        parse("<@&1> @here <:wave:2> <a:dance:3>", all, ""),
        json!({"nodes":[
            {"type":"Mention","kind":{"kind":"Role","id":"1"}},
            {"type":"Text","content":" "},
            {"type":"Mention","kind":{"kind":"Here"}},
            {"type":"Text","content":" "},
            {"type":"Emoji","kind":{"kind":"Custom","name":"wave","id":"2","animated":false}},
            {"type":"Text","content":" "},
            {"type":"Emoji","kind":{"kind":"Custom","name":"dance","id":"3","animated":true}}
        ]})
    );
    // Without the user-mention flag the syntax stays text.
    assert_eq!(
        parse("<@123>", 0, ""),
        json!({"nodes":[{"type":"Text","content":"<@123>"}]})
    );
}

#[test]
fn blocks() {
    let all = ParserFlags::ALL;
    let alert = nodes("> [!NOTE]\n> hi", all, "");
    assert!(matches!(&alert[..], [Node::Alert { .. }]), "{alert:?}");
    let sub = nodes("-# small", all, "");
    assert!(matches!(&sub[..], [Node::Subtext { .. }]), "{sub:?}");
    let table = nodes("| a | b |\n| - | - |\n| 1 | 2 |", all, "");
    assert!(
        matches!(&table[..], [Node::Table { rows, .. }] if rows.len() == 1),
        "{table:?}"
    );
    let auto = nodes("see https://example.com", all, "");
    assert!(
        matches!(&auto[..], [Node::Text { .. }, Node::Link { url, .. }] if url == "https://example.com"),
        "{auto:?}"
    );
}
