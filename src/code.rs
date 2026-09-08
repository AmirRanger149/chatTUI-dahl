//! Fenced code block extraction, shared by the copy-code overlay. (The
//! transcript renderer parses fences line by line so it can keep up with
//! streaming, but the copy flow needs whole blocks.)

/// One fenced ``` code block found in a message.
pub struct CodeBlock {
    /// Language info string from the opening fence ("" when absent).
    pub lang: String,
    /// The raw code between the fences.
    pub code: String,
}

/// Extract fenced ``` code blocks in order of appearance. An unclosed fence
/// still yields whatever it contains so far, so streamed code can be copied
/// while it is still being generated.
pub fn extract(src: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, String)> = None;

    for raw in src.lines() {
        if let Some(info) = raw.trim_start().strip_prefix("```") {
            match current.take() {
                Some((lang, code)) => blocks.push(CodeBlock { lang, code }),
                None => {
                    let lang = info
                        .trim()
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    current = Some((lang, String::new()));
                }
            }
            continue;
        }
        if let Some((_, code)) = &mut current {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(raw);
        }
    }
    if let Some((lang, code)) = current {
        blocks.push(CodeBlock { lang, code });
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_multiple_blocks_with_languages() {
        let src = "intro\n```rust\nlet x = 1;\nlet y = 2;\n```\nmiddle\n```\nplain\n```\n";
        let blocks = extract(src);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lang, "rust");
        assert_eq!(blocks[0].code, "let x = 1;\nlet y = 2;");
        assert_eq!(blocks[1].lang, "");
        assert_eq!(blocks[1].code, "plain");
    }

    #[test]
    fn unclosed_fence_still_yields_code() {
        let blocks = extract("```py\nprint(1)\nprint(2)");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lang, "py");
        assert_eq!(blocks[0].code, "print(1)\nprint(2)");
    }

    #[test]
    fn indentation_is_preserved() {
        let blocks = extract("```\n  indented\n    more\n```");
        assert_eq!(blocks[0].code, "  indented\n    more");
    }
}
