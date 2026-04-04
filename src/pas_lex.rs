pub fn skip_brace_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        if bytes[i] == b'}' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

pub fn skip_paren_comment(bytes: &[u8], mut i: usize) -> usize {
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b')' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

pub fn skip_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'\n' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

pub fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                } else {
                    return i + 1;
                }
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

pub fn read_string_literal(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    if bytes.get(start) != Some(&b'\'') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                if bytes.get(i + 1) == Some(&b'\'') {
                    out.push(b'\'');
                    i += 2;
                } else {
                    let value = String::from_utf8_lossy(&out).to_string();
                    return Some((value, i + 1));
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    None
}

pub fn skip_ws_and_comments(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' => i = skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => i = skip_paren_comment(bytes, i + 2),
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = skip_line_comment(bytes, i + 2),
            b'\'' => i = skip_string(bytes, i + 1),
            _ => break,
        }
    }
    i
}

pub fn parse_include_directive(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    match parse_compiler_directive(bytes, start) {
        Some((CompilerDirective::Include(filename), end)) => Some((filename, end)),
        _ => None,
    }
}

#[derive(Copy, Clone)]
enum CommentEnd {
    Brace,
    Paren,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompilerDirective {
    Include(String),
    IfDef(String),
    IfNDef(String),
    IfExpr(String),
    IfOpt(String),
    ElseIfExpr(String),
    Else,
    EndIf,
    UnsupportedAffecting(String),
    Other(String),
}

pub fn parse_compiler_directive(bytes: &[u8], start: usize) -> Option<(CompilerDirective, usize)> {
    if start >= bytes.len() {
        return None;
    }
    match bytes[start] {
        b'{' => parse_compiler_directive_inner(bytes, start + 1, CommentEnd::Brace),
        b'(' if bytes.get(start + 1) == Some(&b'*') => {
            parse_compiler_directive_inner(bytes, start + 2, CommentEnd::Paren)
        }
        _ => None,
    }
}

fn parse_compiler_directive_inner(
    bytes: &[u8],
    mut i: usize,
    end: CommentEnd,
) -> Option<(CompilerDirective, usize)> {
    i = skip_ws(bytes, i);
    if bytes.get(i) != Some(&b'$') {
        return None;
    }
    i += 1;
    i = skip_ws(bytes, i);
    if i >= bytes.len() || !is_ident_start(bytes[i]) {
        return None;
    }
    let (token, next) = read_ident(bytes, i);
    i = next;
    let upper = token.to_ascii_uppercase();

    let directive = match upper.as_str() {
        "I" | "INCLUDE" => {
            i = skip_ws(bytes, i);
            let (filename, next) = read_directive_filename(bytes, i, end)?;
            i = skip_ws(bytes, next);
            CompilerDirective::Include(filename)
        }
        "IFDEF" => {
            i = skip_ws(bytes, i);
            let (symbol, next) = read_directive_symbol(bytes, i, end)?;
            i = skip_ws(bytes, next);
            CompilerDirective::IfDef(symbol)
        }
        "IFNDEF" => {
            i = skip_ws(bytes, i);
            let (symbol, next) = read_directive_symbol(bytes, i, end)?;
            i = skip_ws(bytes, next);
            CompilerDirective::IfNDef(symbol)
        }
        "IF" => {
            i = skip_ws(bytes, i);
            let (expr, next) = read_directive_payload(bytes, i, end)?;
            i = skip_ws(bytes, next);
            CompilerDirective::IfExpr(expr)
        }
        "IFOPT" => {
            i = skip_ws(bytes, i);
            let (expr, next) = read_directive_payload(bytes, i, end)?;
            i = skip_ws(bytes, next);
            CompilerDirective::IfOpt(expr)
        }
        "ELSEIF" => {
            i = skip_ws(bytes, i);
            let (expr, next) = read_directive_payload(bytes, i, end)?;
            i = skip_ws(bytes, next);
            CompilerDirective::ElseIfExpr(expr)
        }
        "ELSE" => {
            i = skip_ws(bytes, i);
            CompilerDirective::Else
        }
        "ENDIF" | "IFEND" => {
            i = skip_ws(bytes, i);
            CompilerDirective::EndIf
        }
        "DEFINE" | "UNDEF" => {
            i = skip_to_comment_end(bytes, i, end);
            CompilerDirective::UnsupportedAffecting(upper)
        }
        _ => {
            i = skip_to_comment_end(bytes, i, end);
            CompilerDirective::Other(upper)
        }
    };

    let end_pos = find_comment_end(bytes, i, end)?;
    Some((directive, end_pos))
}

fn read_directive_filename(bytes: &[u8], mut i: usize, end: CommentEnd) -> Option<(String, usize)> {
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'\'' {
        let (value, next) = read_string_literal(bytes, i)?;
        if value.trim().is_empty() {
            return None;
        }
        return Some((value, next));
    }

    let start = i;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !is_comment_end(bytes, i, end) {
        i += 1;
    }
    if i == start {
        return None;
    }
    let value = String::from_utf8_lossy(&bytes[start..i]).trim().to_string();
    if value.is_empty() {
        return None;
    }
    if value == "+" || value == "-" {
        return None;
    }
    Some((value, i))
}

fn read_directive_payload(bytes: &[u8], i: usize, end: CommentEnd) -> Option<(String, usize)> {
    let start = i;
    let next = skip_to_comment_end(bytes, i, end);
    if next < start {
        return None;
    }
    let value = String::from_utf8_lossy(&bytes[start..next])
        .trim()
        .to_string();
    if value.is_empty() {
        None
    } else {
        Some((value, next))
    }
}

fn find_comment_end(bytes: &[u8], mut i: usize, end: CommentEnd) -> Option<usize> {
    while i < bytes.len() {
        if is_comment_end(bytes, i, end) {
            return match end {
                CommentEnd::Brace => Some(i + 1),
                CommentEnd::Paren => Some(i + 2),
            };
        }
        i += 1;
    }
    None
}

fn skip_to_comment_end(bytes: &[u8], mut i: usize, end: CommentEnd) -> usize {
    while i < bytes.len() && !is_comment_end(bytes, i, end) {
        i += 1;
    }
    i
}

fn is_comment_end(bytes: &[u8], i: usize, end: CommentEnd) -> bool {
    match end {
        CommentEnd::Brace => bytes.get(i) == Some(&b'}'),
        CommentEnd::Paren => bytes.get(i) == Some(&b'*') && bytes.get(i + 1) == Some(&b')'),
    }
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn read_directive_symbol(bytes: &[u8], i: usize, _end: CommentEnd) -> Option<(String, usize)> {
    if i >= bytes.len() || !is_ident_start(bytes[i]) {
        return None;
    }

    let start = i;
    let mut next = i + 1;
    while next < bytes.len() && is_ident_continue(bytes[next]) {
        next += 1;
    }
    if next == start {
        return None;
    }
    Some((
        String::from_utf8_lossy(&bytes[start..next]).to_string(),
        next,
    ))
}

pub fn read_ident(bytes: &[u8], mut i: usize) -> (String, usize) {
    let start = i;
    i += 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    (String::from_utf8_lossy(&bytes[start..i]).to_string(), i)
}

pub fn read_ident_with_dots(bytes: &[u8], i: usize) -> (String, usize) {
    read_ident(bytes, i)
}

pub fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}
