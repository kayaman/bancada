//! Reading Arduino C++ for what it *says*, without believing what it merely
//! *contains*.
//!
//! Every claim the circuit guess makes rests on this module, so its failure
//! mode is the one that matters: seeing a component that is not there. A
//! `#include <Adafruit_BME280.h>` inside a comment, inside an `#if 0`, or
//! inside a string literal is not a sensor on the bench — but a plain
//! substring search cannot tell the difference. `validate_manifest`'s existing
//! scanners ([`crate::circuit`]) accept that risk because their claims are
//! *warnings about the manifest*; a wiring proposal cannot.
//!
//! So the first thing anything here does is take a **code-only view**:
//! comments, string and character literals, and literal `#if 0` blocks are
//! blanked to spaces. Blanking rather than deleting is deliberate — byte
//! offsets and line numbers survive, so a finding can still cite
//! `rel_path:line` and point the user at the real line.
//!
//! This is not a C++ parser and must never grow into one. It has no
//! preprocessor, evaluates no conditions, resolves no includes, and computes
//! no expressions. It recognises a small closed set of shapes and answers
//! "I don't know" for everything else, because in this module a wrong answer
//! is worse than no answer.

/// A code-only view of `src`: same length, same line count, with everything
/// that is not code replaced by spaces.
///
/// Blanked: `//` line comments (including backslash-continued ones), `/* */`
/// block comments, `"string"` and `'c'` literals, and `#if 0` / `#endif`
/// blocks (nesting-aware).
///
/// Newlines are always preserved so `line_of` stays exact.
///
/// Known limitation, deliberately not handled: C++11 raw string literals
/// (`R"(...)"`). They are vanishingly rare in Arduino sketches, and the
/// failure mode is confined — a raw string's contents stay visible, which can
/// only cause the scanner to *see* something, never to miss real code. Any
/// component it invents from one still has to survive the recipe table.
pub fn code_only(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = bytes.to_vec();
    blank_comments_and_literals(bytes, &mut out);
    // Second pass over the already-blanked view, so an `#if 0` written inside
    // a comment cannot start a skip.
    blank_if_zero_blocks(&mut out);
    // Every replacement is a single ASCII byte swapped for a space, and
    // multi-byte UTF-8 sequences are only ever copied or blanked whole, so the
    // result is still valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// Blank one byte, keeping line structure intact.
fn blank(out: &mut [u8], i: usize) {
    if out[i] != b'\n' && out[i] != b'\r' {
        out[i] = b' ';
    }
}

fn blank_comments_and_literals(bytes: &[u8], out: &mut [u8]) {
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        // Line comment, including backslash-continued ones.
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n {
                if bytes[i] == b'\n' {
                    // A backslash immediately before the newline splices the
                    // next line into this comment.
                    let mut k = i;
                    if k > 0 && bytes[k - 1] == b'\r' {
                        k -= 1;
                    }
                    if k > 0 && bytes[k - 1] == b'\\' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                blank(out, i);
                i += 1;
            }
            continue;
        }
        // Block comment.
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            blank(out, i);
            blank(out, i + 1);
            i += 2;
            while i < n {
                if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    blank(out, i);
                    blank(out, i + 1);
                    i += 2;
                    break;
                }
                blank(out, i);
                i += 1;
            }
            continue;
        }
        // String literal.
        if bytes[i] == b'"' {
            blank(out, i);
            i += 1;
            while i < n {
                if bytes[i] == b'\\' && i + 1 < n {
                    blank(out, i);
                    blank(out, i + 1);
                    i += 2;
                    continue;
                }
                let closing = bytes[i] == b'"';
                blank(out, i);
                i += 1;
                if closing {
                    break;
                }
            }
            continue;
        }
        // Character literal — but only when it really is one. Requiring either
        // a single character or a backslash escape keeps a C++14 digit
        // separator (`1'000'000`) from opening a literal that would blank real
        // code after it.
        if bytes[i] == b'\'' {
            if let Some(end) = char_literal_end(bytes, i) {
                for k in i..=end {
                    blank(out, k);
                }
                i = end + 1;
                continue;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
}

/// The index of the closing quote of a character literal starting at `open`,
/// or `None` when this quote is not one.
fn char_literal_end(bytes: &[u8], open: usize) -> Option<usize> {
    let n = bytes.len();
    let first = *bytes.get(open + 1)?;
    if first == b'\n' {
        return None;
    }
    if first == b'\\' {
        // '\n', '\0', '\x41' — bounded so a stray quote cannot run away.
        let mut k = open + 2;
        while k < n && k <= open + 6 {
            if bytes[k] == b'\n' {
                return None;
            }
            if bytes[k] == b'\'' {
                return Some(k);
            }
            k += 1;
        }
        return None;
    }
    if bytes.get(open + 2) == Some(&b'\'') {
        return Some(open + 2);
    }
    None
}

/// Blank `#if 0` … `#endif` regions, nesting-aware.
///
/// `#else`/`#elif` at the outermost skipped level ends the skip: that branch is
/// the one the compiler keeps.
fn blank_if_zero_blocks(out: &mut [u8]) {
    let mut depth = 0usize;
    let mut line_start = 0usize;
    let mut i = 0usize;
    while i <= out.len() {
        let at_end = i == out.len();
        if !at_end && out[i] != b'\n' {
            i += 1;
            continue;
        }
        let line = &out[line_start..i];
        let trimmed: Vec<u8> = line
            .iter()
            .copied()
            .skip_while(|b| b.is_ascii_whitespace())
            .collect();
        let directive = directive_of(&trimmed);
        let mut blank_this_line = depth > 0;
        match directive {
            Some(Directive::IfZero) if depth == 0 => {
                depth = 1;
                blank_this_line = true;
            }
            Some(Directive::If) if depth > 0 => depth += 1,
            Some(Directive::ElseLike) if depth == 1 => {
                depth = 0;
                blank_this_line = true;
            }
            Some(Directive::Endif) if depth > 0 => {
                depth -= 1;
                blank_this_line = true;
            }
            _ => {}
        }
        if blank_this_line {
            for k in line_start..i {
                blank(out, k);
            }
        }
        if at_end {
            break;
        }
        i += 1;
        line_start = i;
    }
}

enum Directive {
    IfZero,
    If,
    ElseLike,
    Endif,
}

fn directive_of(trimmed: &[u8]) -> Option<Directive> {
    let rest = trimmed.strip_prefix(b"#")?;
    let rest: Vec<u8> = rest
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace())
        .collect();
    let word_end = rest
        .iter()
        .position(|b| b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let word = &rest[..word_end];
    let tail: Vec<u8> = rest[word_end..]
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    match word {
        b"if" if tail == b"0" => Some(Directive::IfZero),
        b"if" | b"ifdef" | b"ifndef" => Some(Directive::If),
        b"else" | b"elif" => Some(Directive::ElseLike),
        b"endif" => Some(Directive::Endif),
        _ => None,
    }
}

/// Where a piece of evidence was read, precisely enough to quote back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSite {
    /// Slash-separated, relative to the project root.
    pub rel_path: String,
    /// 1-based.
    pub line: u32,
    /// The line itself, trimmed — shown to the user verbatim.
    pub text: String,
}

/// A name the project's own source binds to an integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub value: i64,
    pub site: SourceSite,
    /// This `#define` sits inside an `#ifndef` for its own name, so it only
    /// takes effect when the core has not already defined the symbol.
    ///
    /// Load-bearing on the ESP32-S3: `blink`'s `#ifndef LED_BUILTIN / #define
    /// LED_BUILTIN 2` never fires there, because the core's variant header
    /// defines `LED_BUILTIN` as the addressable RGB LED. Believing the `2`
    /// would put a wrong pin in a wiring proposal.
    pub guarded: bool,
}

/// Why a name did not fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unfoldable {
    /// Defined more than once with different values. Both sites are reported;
    /// picking one would be a guess, and this module does not guess.
    Ambiguous(Vec<SourceSite>),
    /// Never defined as a plain integer anywhere in the project's own source.
    Unknown,
}

/// Every integer constant the project binds, in a **code-only** view of one
/// file (call [`code_only`] first).
///
/// Recognises exactly three shapes:
/// - `#define NAME 12` / `#define NAME 0x0C`
/// - `const <int type> NAME = 12;`
/// - `constexpr <int type> NAME = 12;` (with or without a leading `static`)
///
/// Everything else is left unbound on purpose: arithmetic (`(BASE + 1)`),
/// name-to-name aliases (`#define LED PIN_A`), function-like macros, arrays,
/// and enum constants. Resolving those is a preprocessor and a constant
/// evaluator, and a half-built one would be confidently wrong.
pub fn definitions(rel_path: &str, code: &str) -> Vec<Definition> {
    let mut out = Vec::new();
    // Names whose `#ifndef` guard is currently open, innermost last.
    let mut guards: Vec<String> = Vec::new();
    for (index, raw) in code.lines().enumerate() {
        let line = raw.trim();
        let site = || SourceSite {
            rel_path: rel_path.to_string(),
            line: index as u32 + 1,
            text: line.to_string(),
        };
        if let Some(rest) = directive_tail(line, "ifndef") {
            guards.push(rest.split_whitespace().next().unwrap_or("").to_string());
            continue;
        }
        if directive_tail(line, "ifdef").is_some() || directive_tail(line, "if").is_some() {
            // Push a placeholder so the matching #endif pops the right level.
            guards.push(String::new());
            continue;
        }
        if directive_tail(line, "endif").is_some() {
            guards.pop();
            continue;
        }
        if let Some(rest) = directive_tail(line, "define") {
            let mut parts = rest.split_whitespace();
            let Some(name) = parts.next() else { continue };
            // `#define MAX(a,b) ...` is a macro, not a constant.
            if !is_ident(name) {
                continue;
            }
            let tail: Vec<&str> = parts.collect();
            if tail.len() != 1 {
                continue;
            }
            if let Some(value) = parse_int(tail[0]) {
                out.push(Definition {
                    name: name.to_string(),
                    value,
                    site: site(),
                    guarded: guards.iter().any(|g| g == name),
                });
            }
            continue;
        }
        if let Some((name, value)) = parse_const_declaration(line) {
            out.push(Definition {
                name,
                value,
                site: site(),
                guarded: false,
            });
        }
    }
    out
}

/// Resolve `name` against everything the project defines.
pub fn fold<'a>(defs: &'a [Definition], name: &str) -> Result<&'a Definition, Unfoldable> {
    let matches: Vec<&Definition> = defs.iter().filter(|d| d.name == name).collect();
    let Some(first) = matches.first() else {
        return Err(Unfoldable::Unknown);
    };
    // The same value written twice is not a disagreement — a guarded define
    // repeated across two .ino files is ordinary. Different values are.
    if matches.iter().any(|d| d.value != first.value) {
        return Err(Unfoldable::Ambiguous(
            matches.iter().map(|d| d.site.clone()).collect(),
        ));
    }
    Ok(first)
}

/// The text after `#<word>` on a directive line, if this line is that directive.
pub(crate) fn directive_tail<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let rest = line.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix(word)?;
    // `#ifdef` must not match a request for `#if`.
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(rest.trim())
}

pub(crate) fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `12`, `-1`, `0x0C` — and nothing else. No suffixes, no expressions.
pub(crate) fn parse_int(token: &str) -> Option<i64> {
    let (negative, digits) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token.strip_prefix('+').unwrap_or(token)),
    };
    let value = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()?
    } else {
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        digits.parse::<i64>().ok()?
    };
    Some(if negative { -value } else { value })
}

/// `const int NAME = 12;` / `constexpr uint8_t NAME = 0x08;`, single declarator.
fn parse_const_declaration(line: &str) -> Option<(String, i64)> {
    let body = line.strip_suffix(';')?;
    let mut words = body.split_whitespace().peekable();
    if words.peek() == Some(&"static") {
        words.next();
    }
    match words.next()? {
        "const" | "constexpr" => {}
        _ => return None,
    }
    let rest: Vec<&str> = words.collect();
    // <type…> NAME = VALUE — the `=` splits it, and everything before the `=`
    // must end in the name.
    let eq = rest.iter().position(|w| *w == "=")?;
    if eq < 1 || rest.len() != eq + 2 {
        return None;
    }
    let name = rest[eq - 1];
    if !is_ident(name) {
        return None;
    }
    // An array or reference declarator is not a plain integer constant.
    if rest[..eq]
        .iter()
        .any(|w| w.contains('[') || w.contains('&'))
    {
        return None;
    }
    let value = parse_int(rest[eq + 1])?;
    Some((name.to_string(), value))
}

/// One argument of a call or constructor, as written.
///
/// The distinction that matters downstream is `Int` (a pin the sketch states
/// outright) versus `Ident` (a name that may or may not fold) versus
/// everything else, which is never a pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Int(i64),
    Ident(String),
    Other(String),
}

/// A call to one of the [`PIN_CALLS`] functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub args: Vec<Arg>,
    pub site: SourceSite,
}

/// An object declaration: `DHT dht(DHTPIN, DHT22);`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantiation {
    pub class_name: String,
    pub var: String,
    pub args: Vec<Arg>,
    pub site: SourceSite,
}

/// An `#include`, reduced to its basename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    pub header: String,
    pub site: SourceSite,
}

/// Everything one project's own sources say about pins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub includes: Vec<Include>,
    pub calls: Vec<Call>,
    pub instantiations: Vec<Instantiation>,
    pub definitions: Vec<Definition>,
}

/// The calls whose arguments can name a pin.
///
/// A **closed** list on purpose. Scanning every call in a sketch and guessing
/// which argument is a pin would invent wiring from `delay(4)`.
pub const PIN_CALLS: &[&str] = &[
    "pinMode",
    "digitalWrite",
    "digitalRead",
    "analogRead",
    "analogWrite",
    "ledcAttach",
    "ledcAttachPin",
    "attachInterrupt",
    "touchRead",
    "dacWrite",
    "Wire.begin",
    "SPI.begin",
];

/// Type and storage keywords that can never be a component class.
const NOT_A_CLASS: &[&str] = &[
    "void",
    "int",
    "char",
    "bool",
    "float",
    "double",
    "long",
    "short",
    "signed",
    "unsigned",
    "const",
    "constexpr",
    "static",
    "volatile",
    "extern",
    "inline",
    "return",
    "if",
    "else",
    "while",
    "for",
    "switch",
    "case",
    "do",
    "struct",
    "class",
    "enum",
    "union",
    "typedef",
    "namespace",
    "using",
    "template",
    "public",
    "private",
    "protected",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "size_t",
    "byte",
    "word",
    "boolean",
];

/// Scan one file's text. Applies [`code_only`] itself, so callers pass raw
/// source.
pub fn scan_file(rel_path: &str, text: &str) -> Scan {
    let code = code_only(text);
    Scan {
        includes: includes_of(rel_path, &code),
        calls: calls_of(rel_path, &code),
        instantiations: instantiations_of(rel_path, &code),
        definitions: definitions(rel_path, &code),
    }
}

/// Scan a project's **own** sources — [`crate::circuit::SourceRole::Sketch`]
/// only.
///
/// Vendored libraries and their bundled examples are deliberately excluded: an
/// `Adafruit_BME280/examples/bme280test.ino` sitting in the project would
/// otherwise manufacture a sensor the user does not own.
pub fn scan(files: &[crate::circuit::SourceFile]) -> Scan {
    let mut out = Scan::default();
    for file in files
        .iter()
        .filter(|f| f.role == crate::circuit::SourceRole::Sketch)
    {
        let one = scan_file(&file.rel_path, &file.text);
        out.includes.extend(one.includes);
        out.calls.extend(one.calls);
        out.instantiations.extend(one.instantiations);
        out.definitions.extend(one.definitions);
    }
    out
}

fn site_at(rel_path: &str, code: &str, offset: usize) -> SourceSite {
    let line = line_of(code, offset);
    let text = code
        .lines()
        .nth(line as usize - 1)
        .unwrap_or("")
        .trim()
        .to_string();
    SourceSite {
        rel_path: rel_path.to_string(),
        line,
        text,
    }
}

fn includes_of(rel_path: &str, code: &str) -> Vec<Include> {
    let mut out = Vec::new();
    for (index, raw) in code.lines().enumerate() {
        let line = raw.trim();
        let Some(rest) = directive_tail(line, "include") else {
            continue;
        };
        // `code_only` blanks `"quoted"` forms, so only the `<>` form survives
        // with its name intact; the quoted form leaves the angle-free remains.
        let header = rest
            .trim_start_matches(['<', '"'])
            .trim_end_matches(['>', '"'])
            .trim();
        if header.is_empty() {
            continue;
        }
        let base = header.rsplit('/').next().unwrap_or(header);
        out.push(Include {
            header: base.to_string(),
            site: SourceSite {
                rel_path: rel_path.to_string(),
                line: index as u32 + 1,
                text: line.to_string(),
            },
        });
    }
    out
}

/// The byte range of a balanced parenthesised group starting at `open`.
fn balanced(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split on top-level commas only, so nested calls stay whole.
fn split_args(inner: &str) -> Vec<Arg> {
    if inner.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in inner.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                out.push(classify_arg(&current));
                current.clear();
            }
            _ => current.push(c),
        }
    }
    out.push(classify_arg(&current));
    out
}

fn classify_arg(raw: &str) -> Arg {
    let token = raw.trim();
    if let Some(value) = parse_int(token) {
        return Arg::Int(value);
    }
    if is_ident(token) {
        return Arg::Ident(token.to_string());
    }
    Arg::Other(token.to_string())
}

/// Does the identifier ending at `end` have a word boundary before it?
fn boundary_before(bytes: &[u8], start: usize) -> bool {
    start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_')
}

fn calls_of(rel_path: &str, code: &str) -> Vec<Call> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    for name in PIN_CALLS {
        let mut from = 0usize;
        while let Some(rel) = code[from..].find(name) {
            let start = from + rel;
            from = start + name.len();
            if !boundary_before(bytes, start) {
                continue;
            }
            let after = start + name.len();
            let paren = match code[after..].find(|c: char| !c.is_whitespace()) {
                Some(offset) if code.as_bytes()[after + offset] == b'(' => after + offset,
                _ => continue,
            };
            let Some(close) = balanced(bytes, paren) else {
                continue;
            };
            out.push(Call {
                name: (*name).to_string(),
                args: split_args(&code[paren + 1..close]),
                site: site_at(rel_path, code, start),
            });
        }
    }
    out.sort_by_key(|c| (c.site.line, c.name.clone()));
    out
}

fn instantiations_of(rel_path: &str, code: &str) -> Vec<Instantiation> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') || !boundary_before(bytes, i) {
            i += 1;
            continue;
        }
        let class_end = ident_end(bytes, i);
        let class_name = &code[i..class_end];
        if NOT_A_CLASS.contains(&class_name) {
            i = class_end;
            continue;
        }
        // Require whitespace, then a second identifier: `DHT dht`.
        let mut k = class_end;
        while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        if k == class_end || k >= bytes.len() {
            i = class_end;
            continue;
        }
        if !(bytes[k].is_ascii_alphabetic() || bytes[k] == b'_') {
            i = class_end;
            continue;
        }
        let var_end = ident_end(bytes, k);
        let var = &code[k..var_end];
        let mut j = var_end;
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        let (args, consumed) = match bytes.get(j) {
            Some(b';') => (Vec::new(), var_end),
            Some(b'(') => {
                let Some(close) = balanced(bytes, j) else {
                    i = class_end;
                    continue;
                };
                let inner = &code[j + 1..close];
                // `int f(int x);` — a parameter list, not a constructor.
                if inner
                    .split(',')
                    .any(|p| p.split_whitespace().any(|w| NOT_A_CLASS.contains(&w)))
                {
                    i = close;
                    continue;
                }
                (split_args(inner), close)
            }
            _ => {
                i = class_end;
                continue;
            }
        };
        out.push(Instantiation {
            class_name: class_name.to_string(),
            var: var.to_string(),
            args,
            site: site_at(rel_path, code, i),
        });
        i = consumed.max(class_end);
    }
    out
}

fn ident_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    end
}

/// The 1-based line number containing byte `offset`.
pub fn line_of(src: &str, offset: usize) -> u32 {
    let end = offset.min(src.len());
    1 + src.as_bytes()[..end]
        .iter()
        .filter(|b| **b == b'\n')
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case this module exists for: a sensor that was wired once, then
    /// commented out. A substring search for the include still finds it.
    const COMMENTED_OUT_WIRE: &str = "\
// #include <Adafruit_BME280.h>
// Adafruit_BME280 bme;
void setup() { pinMode(4, OUTPUT); }
";

    /// The same trap spelled with the preprocessor. `#if 0` is how people park
    /// code they are not using.
    const IF_ZERO_BME280: &str = "\
#if 0
#include <Adafruit_BME280.h>
Adafruit_BME280 bme;
#endif
void loop() {}
";

    /// Slashes inside a string must not start a comment.
    const STRING_WITH_SLASHES: &str = "\
const char* url = \"http://example.com//x\";
void setup() { pinMode(7, OUTPUT); }
";

    /// An apostrophe inside a block comment must not open a char literal.
    const BLOCK_COMMENT_WITH_QUOTE: &str = "\
/* don't let this \"quote\" swallow the file */
void setup() { pinMode(9, OUTPUT); }
";

    /// A quote as a character literal must not open a string.
    const CHAR_LITERAL_QUOTE: &str = "\
char q = '\"';
void setup() { pinMode(11, OUTPUT); }
";

    /// A line comment ending in a backslash continues onto the next line —
    /// the classic way a "disabled" line silently disables the one below it.
    const CONTINUED_LINE_COMMENT: &str = "\
// disabled \\
#include <DHT.h>
void setup() {}
";

    fn all_fixtures() -> [(&'static str, &'static str); 6] {
        [
            ("COMMENTED_OUT_WIRE", COMMENTED_OUT_WIRE),
            ("IF_ZERO_BME280", IF_ZERO_BME280),
            ("STRING_WITH_SLASHES", STRING_WITH_SLASHES),
            ("BLOCK_COMMENT_WITH_QUOTE", BLOCK_COMMENT_WITH_QUOTE),
            ("CHAR_LITERAL_QUOTE", CHAR_LITERAL_QUOTE),
            ("CONTINUED_LINE_COMMENT", CONTINUED_LINE_COMMENT),
        ]
    }

    #[test]
    fn the_code_only_view_preserves_every_offset_and_line() {
        // The whole point of blanking rather than deleting: a finding cites
        // rel_path:line, and that line number has to be the real one.
        for (name, src) in all_fixtures() {
            let out = code_only(src);
            assert_eq!(out.len(), src.len(), "{name} changed length");
            assert_eq!(
                out.lines().count(),
                src.lines().count(),
                "{name} changed line count"
            );
            assert_eq!(
                out.match_indices('\n').count(),
                src.match_indices('\n').count(),
                "{name} lost a newline"
            );
        }
    }

    #[test]
    fn a_commented_out_sensor_is_not_in_the_code_view() {
        let out = code_only(COMMENTED_OUT_WIRE);
        assert!(!out.contains("Adafruit_BME280"), "{out:?}");
        assert!(out.contains("pinMode(4, OUTPUT)"), "{out:?}");
    }

    #[test]
    fn an_if_zero_block_is_not_in_the_code_view() {
        let out = code_only(IF_ZERO_BME280);
        assert!(!out.contains("Adafruit_BME280"), "{out:?}");
        assert!(out.contains("void loop()"), "{out:?}");
    }

    #[test]
    fn slashes_inside_a_string_do_not_start_a_comment() {
        let out = code_only(STRING_WITH_SLASHES);
        assert!(!out.contains("example.com"), "{out:?}");
        // The code after the string survived, which is what a lost comment
        // boundary would have destroyed.
        assert!(out.contains("pinMode(7, OUTPUT)"), "{out:?}");
    }

    #[test]
    fn a_quote_inside_a_block_comment_does_not_swallow_the_file() {
        let out = code_only(BLOCK_COMMENT_WITH_QUOTE);
        assert!(!out.contains("don't"), "{out:?}");
        assert!(out.contains("pinMode(9, OUTPUT)"), "{out:?}");
    }

    #[test]
    fn a_quote_as_a_char_literal_does_not_open_a_string() {
        let out = code_only(CHAR_LITERAL_QUOTE);
        assert!(out.contains("char q ="), "{out:?}");
        assert!(out.contains("pinMode(11, OUTPUT)"), "{out:?}");
    }

    #[test]
    fn a_backslash_continued_line_comment_swallows_the_next_line() {
        // The include really is commented out here — believing otherwise
        // would invent a DHT sensor from a disabled line.
        let out = code_only(CONTINUED_LINE_COMMENT);
        assert!(!out.contains("DHT.h"), "{out:?}");
        assert!(out.contains("void setup()"), "{out:?}");
    }

    #[test]
    fn a_live_conditional_is_left_alone() {
        // i2c_scan.ino.tmpl's real shape. Only a literal `#if 0` is a skip;
        // blanking a live `#if defined(...)` would hide the pins it guards.
        let src = "\
#if defined(ARDUINO_ARCH_ESP32)
const int PIN_SDA = -1;
#endif
";
        let out = code_only(src);
        assert!(out.contains("PIN_SDA"), "{out:?}");
        assert!(out.contains("ARDUINO_ARCH_ESP32"), "{out:?}");
    }

    #[test]
    fn the_live_branch_of_an_if_zero_else_survives() {
        // `#if 0 / #else` means the else branch is what compiles. Blanking it
        // would lose a real component rather than invent a fake one — the safer
        // direction, but still wrong.
        let src = "\
#if 0
#include <Adafruit_BME280.h>
#else
#include <DHT.h>
#endif
";
        let out = code_only(src);
        assert!(!out.contains("Adafruit_BME280"), "{out:?}");
        assert!(out.contains("DHT.h"), "{out:?}");
    }

    #[test]
    fn a_conditional_nested_inside_if_zero_does_not_end_the_skip_early() {
        let src = "\
#if 0
#ifdef FOO
#include <Adafruit_BME280.h>
#endif
#include <DHT.h>
#endif
void setup() {}
";
        let out = code_only(src);
        assert!(!out.contains("Adafruit_BME280"), "{out:?}");
        assert!(
            !out.contains("DHT.h"),
            "inner #endif ended the skip: {out:?}"
        );
        assert!(out.contains("void setup()"), "{out:?}");
    }

    #[test]
    fn a_digit_separator_is_not_a_character_literal() {
        // C++14 `1'000'000`. Treating the quote as a literal would blank the
        // code after it — including, in the worst case, a pinMode call.
        let src = "const long baud = 1'000'000;\nvoid setup() { pinMode(5, OUTPUT); }\n";
        let out = code_only(src);
        assert!(out.contains("pinMode(5, OUTPUT)"), "{out:?}");
        assert!(out.contains("1'000'000"), "{out:?}");
    }

    #[test]
    fn escaped_quotes_do_not_end_a_string_early() {
        // The escaped quote must not close the literal early and leave the
        // rest of the string looking like code.
        let src =
            "const char* s = \"Adafruit_BME280\\\" bme\";\nvoid setup() { pinMode(6, OUTPUT); }\n";
        let out = code_only(src);
        assert!(!out.contains("Adafruit_BME280"), "{out:?}");
        assert!(!out.contains("bme"), "{out:?}");
        assert!(out.contains("pinMode(6, OUTPUT)"), "{out:?}");
    }

    fn defs(src: &str) -> Vec<Definition> {
        definitions("sketch.ino", &code_only(src))
    }

    #[test]
    fn a_plain_define_folds() {
        let d = defs("#define LED_BUILTIN 2\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "LED_BUILTIN");
        assert_eq!(d[0].value, 2);
        assert_eq!(d[0].site.line, 1);
        assert!(!d[0].guarded);
    }

    #[test]
    fn a_define_guarded_by_its_own_ifndef_is_marked_guarded() {
        // blink.ino.tmpl. On the ESP32-S3 the core defines LED_BUILTIN itself,
        // so this `2` never takes effect — the flag is what stops a proposal
        // claiming GPIO2 there.
        let d = defs("#ifndef LED_BUILTIN\n#define LED_BUILTIN 2\n#endif\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].value, 2);
        assert!(d[0].guarded, "{d:?}");
    }

    #[test]
    fn a_define_guarded_by_a_different_name_is_not_marked_guarded() {
        let d = defs("#ifndef SOMETHING_ELSE\n#define LED_BUILTIN 2\n#endif\n");
        assert_eq!(d.len(), 1);
        assert!(!d[0].guarded, "{d:?}");
    }

    #[test]
    fn const_and_constexpr_declarations_fold_including_negatives_and_hex() {
        // `const int PIN_SDA = -1;` is i2c_scan.ino.tmpl's sentinel: it folds
        // to -1 here, and is rejected as a pin later, with its own message.
        let d =
            defs("const int PIN_SDA = -1;\nconstexpr uint8_t P = 0x08;\nstatic const int Q = 7;\n");
        let found: Vec<(&str, i64)> = d.iter().map(|x| (x.name.as_str(), x.value)).collect();
        assert_eq!(found, vec![("PIN_SDA", -1), ("P", 8), ("Q", 7)]);
    }

    #[test]
    fn the_shapes_that_would_need_a_preprocessor_do_not_fold() {
        let src = "\
#define ALIAS OTHER_NAME
#define MAX(a, b) ((a) > (b) ? (a) : (b))
#define SUM (BASE + 1)
#define EMPTY
const int PINS[] = {4, 5};
const int &REF = other;
const char* NAME = \"x\";
";
        assert!(defs(src).is_empty(), "{:?}", defs(src));
    }

    #[test]
    fn a_symbol_defined_twice_with_different_values_is_ambiguous() {
        let d = defs("#define PIN 4\n#define PIN 5\n");
        match fold(&d, "PIN") {
            Err(Unfoldable::Ambiguous(sites)) => {
                assert_eq!(sites.len(), 2, "both sites must be named: {sites:?}");
                assert_eq!(sites[0].line, 1);
                assert_eq!(sites[1].line, 2);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn the_same_value_written_twice_is_not_a_disagreement() {
        // A guarded define repeated across two .ino files is ordinary.
        let d = defs("#define PIN 4\n#define PIN 4\n");
        assert_eq!(fold(&d, "PIN").unwrap().value, 4);
    }

    #[test]
    fn an_undefined_name_is_unknown_rather_than_zero() {
        let d = defs("#define OTHER 1\n");
        assert_eq!(fold(&d, "ANALOG_PIN"), Err(Unfoldable::Unknown));
    }

    #[test]
    fn a_define_inside_a_comment_or_if_zero_never_reaches_the_folder() {
        // Composed with code_only — the whole reason `defs` goes through it.
        assert!(defs("// #define PIN 4\n").is_empty());
        assert!(defs("#if 0\n#define PIN 4\n#endif\n").is_empty());
    }

    #[test]
    fn a_trailing_comment_does_not_stop_a_define_folding() {
        // analog_plot.ino.tmpl writes `#define ANALOG_PIN A0  // most boards`.
        let d = defs("#define SAMPLE 20  // milliseconds\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].value, 20);
        assert_eq!(d[0].site.text, "#define SAMPLE 20");
    }

    #[test]
    fn an_alias_to_a_core_symbol_does_not_fold() {
        // analog_plot's real line: `#define ANALOG_PIN A0`. A0 is a core
        // variant symbol, not an integer this project states.
        let d = defs("#define ANALOG_PIN A0\n");
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(fold(&d, "ANALOG_PIN"), Err(Unfoldable::Unknown));
    }

    #[test]
    fn includes_are_reduced_to_a_basename() {
        let s = scan_file("a.ino", "#include <Wire.h>\n#include <Adafruit/BME280.h>\n");
        let headers: Vec<&str> = s.includes.iter().map(|i| i.header.as_str()).collect();
        assert_eq!(headers, vec!["Wire.h", "BME280.h"]);
        assert_eq!(s.includes[0].site.line, 1);
    }

    #[test]
    fn a_pin_call_captures_its_arguments_positionally() {
        let s = scan_file("a.ino", "void setup() {\n  pinMode(LED, OUTPUT);\n}\n");
        assert_eq!(s.calls.len(), 1);
        assert_eq!(s.calls[0].name, "pinMode");
        assert_eq!(
            s.calls[0].args,
            vec![Arg::Ident("LED".into()), Arg::Ident("OUTPUT".into())]
        );
        assert_eq!(s.calls[0].site.line, 2);
    }

    #[test]
    fn a_literal_pin_argument_is_an_int() {
        let s = scan_file("a.ino", "void setup() { pinMode(4, OUTPUT); }\n");
        assert_eq!(s.calls[0].args[0], Arg::Int(4));
    }

    #[test]
    fn a_call_name_that_is_a_suffix_of_another_identifier_is_not_a_call() {
        // The same word-boundary discipline as circuit.rs's raw-GPIO scanner.
        let s = scan_file("a.ino", "void setup() { my_pinMode(4, OUTPUT); }\n");
        assert!(s.calls.is_empty(), "{:?}", s.calls);
    }

    #[test]
    fn whitespace_and_nesting_do_not_hide_a_call() {
        let s = scan_file(
            "a.ino",
            "void setup() { analogWrite (LED, map(x, 0, 9, 0, 255)); }\n",
        );
        assert_eq!(s.calls.len(), 1);
        // The nested call stays one argument rather than splitting on its commas.
        assert_eq!(s.calls[0].args.len(), 2);
        assert_eq!(s.calls[0].args[0], Arg::Ident("LED".into()));
    }

    #[test]
    fn a_dotted_bus_call_is_found() {
        let s = scan_file("a.ino", "void setup() { Wire.begin(8, 9); }\n");
        assert_eq!(s.calls.len(), 1);
        assert_eq!(s.calls[0].name, "Wire.begin");
        assert_eq!(s.calls[0].args, vec![Arg::Int(8), Arg::Int(9)]);
    }

    #[test]
    fn an_object_declaration_is_an_instantiation_with_or_without_arguments() {
        let s = scan_file(
            "a.ino",
            "Adafruit_BME280 bme;\nDHT dht(DHTPIN, DHT22);\nAdafruit_NeoPixel strip(60, PIN, NEO_GRB);\n",
        );
        let found: Vec<(&str, &str, usize)> = s
            .instantiations
            .iter()
            .map(|i| (i.class_name.as_str(), i.var.as_str(), i.args.len()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("Adafruit_BME280", "bme", 0),
                ("DHT", "dht", 2),
                ("Adafruit_NeoPixel", "strip", 3),
            ]
        );
        // The pin is argument 1 for NeoPixel and argument 0 for DHT — which is
        // exactly why the recipe table carries an index rather than assuming.
        assert_eq!(s.instantiations[1].args[0], Arg::Ident("DHTPIN".into()));
        assert_eq!(s.instantiations[2].args[1], Arg::Ident("PIN".into()));
    }

    #[test]
    fn a_function_declaration_is_not_an_instantiation() {
        let s = scan_file(
            "a.ino",
            "void loop();\nint readSensor(int pin);\nstatic void helper(uint8_t x);\n",
        );
        assert!(s.instantiations.is_empty(), "{:?}", s.instantiations);
    }

    #[test]
    fn an_instantiation_inside_a_comment_or_if_zero_is_not_seen() {
        assert!(scan_file("a.ino", "// DHT dht(4, DHT22);\n")
            .instantiations
            .is_empty());
        let parked = scan_file("a.ino", "#if 0\nDHT dht(4, DHT22);\n#endif\n");
        assert!(parked.instantiations.is_empty(), "{:?}", parked);
        assert!(parked.includes.is_empty());
    }

    #[test]
    fn scanning_a_project_ignores_vendored_and_example_sources() {
        use crate::circuit::{SourceFile, SourceRole};
        let files = vec![
            SourceFile {
                rel_path: "a.ino".into(),
                text: "#include <Wire.h>\n".into(),
                role: SourceRole::Sketch,
            },
            SourceFile {
                rel_path: "libraries/Foo/Foo.cpp".into(),
                text: "#include <Adafruit_BME280.h>\nAdafruit_BME280 bme;\n".into(),
                role: SourceRole::Vendored,
            },
            SourceFile {
                rel_path: "examples/Bar/Bar.ino".into(),
                text: "#include <DHT.h>\nDHT dht(4, DHT22);\n".into(),
                role: SourceRole::Example,
            },
        ];
        let s = scan(&files);
        let headers: Vec<&str> = s.includes.iter().map(|i| i.header.as_str()).collect();
        assert_eq!(headers, vec!["Wire.h"]);
        assert!(s.instantiations.is_empty(), "{:?}", s.instantiations);
    }

    #[test]
    fn line_of_counts_from_one() {
        let src = "a\nb\nc\n";
        assert_eq!(line_of(src, 0), 1);
        assert_eq!(line_of(src, 2), 2);
        assert_eq!(line_of(src, 4), 3);
        // Out-of-range offsets clamp rather than panic; callers derive offsets
        // from the code view, which is the same length as the source.
        assert_eq!(line_of(src, 999), 4);
    }
}
