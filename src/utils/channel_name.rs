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
}
