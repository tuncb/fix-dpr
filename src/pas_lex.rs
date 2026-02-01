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
