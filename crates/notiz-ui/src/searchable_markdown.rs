pub fn case_insensitive_ranges(value: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let query_chars = query.chars().count();
    let boundaries = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect::<Vec<_>>();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start + query_chars < boundaries.len() {
        let end = start + query_chars;
        let range = boundaries[start]..boundaries[end];
        if value[range.clone()].to_lowercase() == query_lower {
            ranges.push(range);
            start = end;
        } else {
            start += 1;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::case_insensitive_ranges;

    #[test]
    fn finds_every_case_insensitive_occurrence() {
        assert_eq!(
            case_insensitive_ranges("Notiz keeps NOTIZ notes", "notiz"),
            vec![0..5, 12..17]
        );
    }

    #[test]
    fn returns_utf8_safe_byte_ranges() {
        let value = "ação e AÇÃO";
        let matches = case_insensitive_ranges(value, "ação");
        assert_eq!(matches, vec![0..6, 9..15]);
        assert_eq!(&value[matches[0].clone()], "ação");
        assert_eq!(&value[matches[1].clone()], "AÇÃO");
    }

    #[test]
    fn ignores_an_empty_query() {
        assert!(case_insensitive_ranges("anything", "  ").is_empty());
    }
}
