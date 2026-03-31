/// Slugify text into a Discord-safe channel name.
/// Lowercase, alphanumeric + dashes, max 40 chars.
pub fn to_channel_name(text: &str) -> String {
    let slug: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive dashes, trim leading/trailing dashes
    let collapsed: String = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let mut result = collapsed;
    if result.len() > 40 {
        result.truncate(40);
        // Don't end on a dash
        result = result.trim_end_matches('-').to_string();
    }
    if result.is_empty() {
        result = "claude-session".to_string();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_slugify() {
        assert_eq!(to_channel_name("Hello World"), "hello-world");
        assert_eq!(to_channel_name("my_project"), "my-project");
        assert_eq!(to_channel_name("  spaces  "), "spaces");
    }

    #[test]
    fn truncates_long_names() {
        let long = "a".repeat(50);
        assert!(to_channel_name(&long).len() <= 40);
    }

    #[test]
    fn empty_string_returns_default() {
        assert_eq!(to_channel_name(""), "claude-session");
    }

    #[test]
    fn only_special_chars_returns_default() {
        assert_eq!(to_channel_name("!!!@@@###"), "claude-session");
    }

    #[test]
    fn collapses_consecutive_dashes() {
        assert_eq!(to_channel_name("a---b"), "a-b");
        assert_eq!(to_channel_name("a   b"), "a-b");
    }

    #[test]
    fn strips_leading_trailing_dashes() {
        assert_eq!(to_channel_name("-hello-"), "hello");
        assert_eq!(to_channel_name("  hello  "), "hello");
    }

    #[test]
    fn unicode_alphanumeric_kept() {
        // Rust's is_alphanumeric() is Unicode-aware, so 'é' passes through
        assert_eq!(to_channel_name("café"), "café");
    }

    #[test]
    fn special_symbols_replaced() {
        assert_eq!(to_channel_name("a+b=c"), "a-b-c");
        assert_eq!(to_channel_name("hello@world.com"), "hello-world-com");
    }

    #[test]
    fn truncation_does_not_end_on_dash() {
        // 39 a's + dash + a = 41 chars; after truncate to 40 the trailing dash is trimmed
        let input = format!("{}-a", "a".repeat(39));
        let result = to_channel_name(&input);
        assert!(result.len() <= 40);
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn numeric_input() {
        assert_eq!(to_channel_name("12345"), "12345");
    }

    #[test]
    fn mixed_case_lowered() {
        assert_eq!(to_channel_name("FooBar"), "foobar");
    }
}
