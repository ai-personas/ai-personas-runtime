//! Exact edits are located against one immutable base before any replacement.
use super::*;

pub(super) fn apply(base: &f::FileRevision, operation: &str, edits: &[p::TextEdit]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(&base.bytes)
        .map_err(|_| held(format!("file_edit {operation}: base is not UTF-8")))?;
    let mut matches = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        let first_char = edit.old_text.chars().next()
            .ok_or_else(|| held(format!("file_edit {operation}.edits[{index}].old_text: empty match")))?;
        let start = text.find(&edit.old_text)
            .ok_or_else(|| held(format!("file_edit {operation}.edits[{index}].old_text: no exact base match")))?;
        // Look after the first character, not after the whole match: overlapping
        // occurrences also make an edit ambiguous. Every offset is a UTF-8 boundary.
        require(!text[start + first_char.len_utf8()..].contains(&edit.old_text),
            &format!("file_edit {operation}.edits[{index}].old_text: multiple exact base matches"))?;
        matches.push((start, start + edit.old_text.len(), index, edit.new_text.as_str()));
    }
    matches.sort_by_key(|m| m.0);
    for pair in matches.windows(2) {
        require(pair[0].1 <= pair[1].0,
            &format!("file_edit {operation}.edits[{}]: overlaps edit {} in the base", pair[1].2, pair[0].2))?;
    }
    let mut result = text.to_owned();
    for (start, end, _, replacement) in matches.into_iter().rev() {
        result.replace_range(start..end, replacement);
    }
    require(result.as_bytes() != base.bytes,
        &format!("file_edit {operation}: edits leave the base bytes unchanged; reuse the exact reference"))?;
    Ok(result.into_bytes())
}
