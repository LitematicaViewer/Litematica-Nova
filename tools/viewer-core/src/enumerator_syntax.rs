// Shared grammar only. Statistics sorts members and the interactive editor
// keeps insertion order; neither consumer should redefine operator precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyntaxError<'a> { WildcardGroup(&'a str), Parentheses }

impl std::fmt::Display for SyntaxError<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
            Self::WildcardGroup(_) => "Wildcard collections must be parenthesized",
            Self::Parentheses => "Unmatched parentheses",
        })
    }
}

pub(crate) fn tokens(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    for (offset, ch) in text.char_indices().filter(|(_, ch)| "()∪|∩&-".contains(*ch)) {
        let value = text[start..offset].trim();
        if !value.is_empty() { result.push(value); }
        start = offset + ch.len_utf8();
        result.push(&text[offset..start]);
    }
    let last = text[start..].trim();
    if !last.is_empty() { result.push(last); }
    result
}

pub(crate) fn wildcard_group<'a>(tokens: &[&'a str], index: usize) -> Result<(), SyntaxError<'a>> {
    let token = tokens[index];
    if token.contains('*') && !(index > 0 && tokens[index - 1] == "(" && tokens.get(index+1) == Some(&")")) {
        return Err(SyntaxError::WildcardGroup(token));
    }
    Ok(())
}

pub(crate) fn precedence(token: &str) -> u8 {
    match token { "&" | "∩" => 2, "|" | "∪" | "-" => 1, _ => 0 }
}

pub(crate) fn postfix<'a>(tokens: &[&'a str]) -> Result<Vec<&'a str>, SyntaxError<'a>> {
    let (mut out, mut operators) = (Vec::new(), Vec::new());
    for (index, &token) in tokens.iter().enumerate() {
        wildcard_group(tokens, index)?;
        match token {
            "(" => operators.push(token),
            ")" => {
                while operators.last().is_some_and(|op| *op != "(") { out.push(operators.pop().unwrap()); }
                operators.pop().ok_or(SyntaxError::Parentheses)?;
            }
            _ if precedence(token) > 0 => {
                while operators.last().is_some_and(|op| precedence(op) >= precedence(token)) { out.push(operators.pop().unwrap()); }
                operators.push(token);
            }
            _ => out.push(token),
        }
    }
    while let Some(op) = operators.pop() {
        if op == "(" { return Err(SyntaxError::Parentheses); }
        out.push(op);
    }
    Ok(out)
}
