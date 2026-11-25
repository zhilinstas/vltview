/// Filter engine for searching and filtering secrets using fuzzy matching
pub struct FilterEngine {
    filter_text: String,
}

impl FilterEngine {
    pub fn new() -> Self {
        Self {
            filter_text: String::new(),
        }
    }

    pub fn set_filter(&mut self, text: String) {
        self.filter_text = text;
    }

    /// Filter items using fuzzy search, returning matches sorted by score (best first)
    pub fn filter(&self, items: &[String]) -> Vec<String> {
        if self.filter_text.is_empty() {
            return items.to_vec();
        }

        let pattern = self.filter_text.to_lowercase();
        let mut scored: Vec<(String, i32)> = items
            .iter()
            .filter_map(|item| {
                fuzzy_match(&pattern, &item.to_lowercase()).map(|score| (item.clone(), score))
            })
            .collect();

        // Sort by score descending (higher = better match)
        scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));

        scored.into_iter().map(|(item, _)| item).collect()
    }
}

impl Default for FilterEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Fuzzy match pattern against text, returning a score if matched
/// Higher scores indicate better matches
fn fuzzy_match(pattern: &str, text: &str) -> Option<i32> {
    if pattern.is_empty() {
        return Some(0);
    }

    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();

    let mut pattern_idx = 0;
    let mut score = 0;
    let mut prev_match_idx: Option<usize> = None;
    let mut first_match_idx: Option<usize> = None;

    for (text_idx, &text_char) in text_chars.iter().enumerate() {
        if pattern_idx < pattern_chars.len() && text_char == pattern_chars[pattern_idx] {
            if first_match_idx.is_none() {
                first_match_idx = Some(text_idx);
            }

            // Bonus for consecutive matches
            if let Some(prev) = prev_match_idx
                && text_idx == prev + 1
            {
                score += 10; // Consecutive character bonus
            }

            // Bonus for matching at word boundaries (after '/', '_', '-', '.')
            if text_idx == 0 {
                score += 15; // Start of string bonus
            } else {
                let prev_char = text_chars[text_idx - 1];
                if prev_char == '/' || prev_char == '_' || prev_char == '-' || prev_char == '.' {
                    score += 12; // Word boundary bonus
                }
            }

            prev_match_idx = Some(text_idx);
            pattern_idx += 1;
            score += 5; // Base score for each matched character
        }
    }

    // All pattern characters must be found
    if pattern_idx == pattern_chars.len() {
        // Bonus for matches near the start
        if let Some(first) = first_match_idx {
            score += (20_i32).saturating_sub(first as i32);
        }

        // Bonus for shorter strings (more precise match)
        score += (50_i32).saturating_sub(text_chars.len() as i32);

        // Bonus for exact match
        if pattern == text {
            score += 100;
        }

        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact() {
        assert!(fuzzy_match("secret", "secret").is_some());
        let score = fuzzy_match("secret", "secret").unwrap();
        assert!(score > 100); // Exact match bonus
    }

    #[test]
    fn test_fuzzy_match_substring() {
        assert!(fuzzy_match("sec", "secret").is_some());
        assert!(fuzzy_match("ret", "secret").is_some());
    }

    #[test]
    fn test_fuzzy_match_scattered() {
        // "abc" should match "aXbXc"
        assert!(fuzzy_match("abc", "aXbXc").is_some());
        // "db" should match "database"
        assert!(fuzzy_match("db", "database").is_some());
    }

    #[test]
    fn test_fuzzy_match_no_match() {
        assert!(fuzzy_match("xyz", "secret").is_none());
        assert!(fuzzy_match("abc", "cba").is_none()); // Wrong order
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        // The filter normalizes to lowercase before matching
        let engine = FilterEngine::new();
        let items = vec!["SECRET".to_string(), "secret".to_string()];

        let mut engine = engine;
        engine.set_filter("sec".to_string());
        let results = engine.filter(&items);

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_filter_sorting() {
        let mut engine = FilterEngine::new();
        engine.set_filter("db".to_string());

        let items = vec![
            "my_database_config".to_string(),
            "db".to_string(),
            "database".to_string(),
            "prod_db_password".to_string(),
        ];

        let results = engine.filter(&items);

        // "db" should be first (exact match)
        assert_eq!(results[0], "db");
        // "database" should be second (starts with pattern)
        assert_eq!(results[1], "database");
    }

    #[test]
    fn test_word_boundary_bonus() {
        // Matches at word boundaries should score higher
        // "sec" in "my_secret" starts after '_' (boundary)
        // "sec" in "mysecret" starts in the middle (no boundary)
        let score_boundary = fuzzy_match("sec", "my_secret").unwrap();
        let score_middle = fuzzy_match("sec", "mysecret").unwrap();
        assert!(score_boundary > score_middle);
    }

    #[test]
    fn test_empty_filter() {
        let engine = FilterEngine::new();
        let items = vec!["a".to_string(), "b".to_string()];
        let results = engine.filter(&items);
        assert_eq!(results, items);
    }

    #[test]
    fn test_path_matching() {
        let mut engine = FilterEngine::new();
        engine.set_filter("pwd".to_string());

        let items = vec![
            "secret/app/password".to_string(),
            "secret/db/pwd".to_string(),
            "passwords/admin".to_string(),
        ];

        let results = engine.filter(&items);
        assert_eq!(results.len(), 3);
        // "pwd" exact match at word boundary should be first
        assert_eq!(results[0], "secret/db/pwd");
    }
}
