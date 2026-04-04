use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::pas_lex::{self, CompilerDirective};
use crate::unit_cache::{self, UnitCache};
use crate::uses_include;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CondExpr {
    True,
    False,
    Symbol(String),
    IfOpt(String),
    Not(Box<CondExpr>),
    And(Vec<CondExpr>),
    Or(Vec<CondExpr>),
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionalUse {
    pub unit_name: String,
    pub in_path: Option<String>,
    pub condition: CondExpr,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssumedValue {
    On,
    Off,
}

#[derive(Clone, Debug, Default)]
pub struct Assumptions {
    values: HashMap<String, AssumedValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalResult {
    Always,
    Never,
    Maybe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregatedConditionalUnit {
    pub name: String,
    pub condition: CondExpr,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConditionBuckets {
    pub unconditional: Vec<String>,
    pub positive: Vec<(String, Vec<String>)>,
    pub negative: Vec<(String, Vec<String>)>,
    pub complex: Vec<(String, String)>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Section {
    None,
    Interface,
    Implementation,
}

#[derive(Clone, Debug)]
struct CondFrame {
    active_branch: CondExpr,
    remaining_branch: CondExpr,
    seen_else: bool,
}

#[derive(Clone, Debug, Default)]
struct ConditionState {
    frames: Vec<CondFrame>,
    sticky_unknowns: Vec<CondExpr>,
    warned_unsupported: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
struct IncludeParseResult {
    entries: Vec<ConditionalUse>,
    ended: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ResolutionSource {
    Project,
    Delphi,
}

enum ResolveByName {
    NotFound,
    Unique {
        path: PathBuf,
        source: ResolutionSource,
    },
    Ambiguous {
        count: usize,
        source: ResolutionSource,
    },
}

pub fn normalize_condition(expr: CondExpr) -> CondExpr {
    match expr {
        CondExpr::True => CondExpr::True,
        CondExpr::False => CondExpr::False,
        CondExpr::Symbol(symbol) => CondExpr::Symbol(symbol.trim().to_ascii_uppercase()),
        CondExpr::IfOpt(symbol) => CondExpr::IfOpt(symbol.trim().to_ascii_uppercase()),
        CondExpr::Unknown(label) => CondExpr::Unknown(label.trim().to_ascii_uppercase()),
        CondExpr::Not(inner) => match normalize_condition(*inner) {
            CondExpr::True => CondExpr::False,
            CondExpr::False => CondExpr::True,
            CondExpr::Not(nested) => *nested,
            other => CondExpr::Not(Box::new(other)),
        },
        CondExpr::And(parts) => normalize_join(parts, true),
        CondExpr::Or(parts) => normalize_join(parts, false),
    }
}

pub fn evaluate_condition(expr: &CondExpr, assumptions: &Assumptions) -> EvalResult {
    match expr {
        CondExpr::True => EvalResult::Always,
        CondExpr::False => EvalResult::Never,
        CondExpr::IfOpt(_) => EvalResult::Maybe,
        CondExpr::Unknown(_) => EvalResult::Maybe,
        CondExpr::Symbol(symbol) => match assumptions.get(symbol) {
            Some(AssumedValue::On) => EvalResult::Always,
            Some(AssumedValue::Off) => EvalResult::Never,
            None => EvalResult::Maybe,
        },
        CondExpr::Not(inner) => match evaluate_condition(inner, assumptions) {
            EvalResult::Always => EvalResult::Never,
            EvalResult::Never => EvalResult::Always,
            EvalResult::Maybe => EvalResult::Maybe,
        },
        CondExpr::And(parts) => {
            let mut maybe = false;
            for part in parts {
                match evaluate_condition(part, assumptions) {
                    EvalResult::Never => return EvalResult::Never,
                    EvalResult::Maybe => maybe = true,
                    EvalResult::Always => {}
                }
            }
            if maybe {
                EvalResult::Maybe
            } else {
                EvalResult::Always
            }
        }
        CondExpr::Or(parts) => {
            let mut maybe = false;
            for part in parts {
                match evaluate_condition(part, assumptions) {
                    EvalResult::Always => return EvalResult::Always,
                    EvalResult::Maybe => maybe = true,
                    EvalResult::Never => {}
                }
            }
            if maybe {
                EvalResult::Maybe
            } else {
                EvalResult::Never
            }
        }
    }
}

pub fn render_condition(expr: &CondExpr) -> String {
    render_condition_with_precedence(expr, 0)
}

pub fn flatten_conditional_uses(uses: &[ConditionalUse], assumptions: &Assumptions) -> Vec<String> {
    let mut flattened = Vec::new();
    for entry in uses {
        if evaluate_condition(&entry.condition, assumptions) == EvalResult::Never {
            continue;
        }
        flattened.push(entry.unit_name.clone());
    }
    flattened
}

pub fn bucket_conditionals(units: &[AggregatedConditionalUnit]) -> ConditionBuckets {
    let mut unconditional = BTreeSet::new();
    let mut positive: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut negative: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut complex = Vec::new();

    for unit in units {
        let normalized = normalize_condition(unit.condition.clone());
        match normalized {
            CondExpr::True => {
                unconditional.insert(unit.name.clone());
            }
            CondExpr::Symbol(symbol) => {
                positive.entry(symbol).or_default().push(unit.name.clone());
            }
            CondExpr::Not(inner) => match *inner {
                CondExpr::Symbol(symbol) => {
                    negative.entry(symbol).or_default().push(unit.name.clone());
                }
                other => complex.push((unit.name.clone(), render_condition(&other_not(other)))),
            },
            other => complex.push((unit.name.clone(), render_condition(&other))),
        }
    }

    let unconditional = unconditional.into_iter().collect();
    let positive = positive
        .into_iter()
        .map(|(symbol, mut names)| {
            names.sort_by_key(|name| name.to_ascii_lowercase());
            names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
            (symbol, names)
        })
        .collect();
    let negative = negative
        .into_iter()
        .map(|(symbol, mut names)| {
            names.sort_by_key(|name| name.to_ascii_lowercase());
            names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
            (symbol, names)
        })
        .collect();
    complex.sort_by(|left, right| {
        left.0
            .to_ascii_lowercase()
            .cmp(&right.0.to_ascii_lowercase())
            .then_with(|| left.1.cmp(&right.1))
    });

    ConditionBuckets {
        unconditional,
        positive,
        negative,
        complex,
    }
}

impl Assumptions {
    #[allow(dead_code)]
    pub fn set(&mut self, symbol: impl AsRef<str>, value: AssumedValue) {
        self.values
            .insert(symbol.as_ref().trim().to_ascii_uppercase(), value);
    }

    pub fn get(&self, symbol: &str) -> Option<AssumedValue> {
        self.values
            .get(&symbol.trim().to_ascii_uppercase())
            .copied()
    }
}

impl ConditionState {
    fn current_condition(&self) -> CondExpr {
        let mut parts = Vec::new();
        for frame in &self.frames {
            parts.push(frame.current_branch());
        }
        parts.extend(self.sticky_unknowns.iter().cloned());
        if parts.is_empty() {
            CondExpr::True
        } else {
            normalize_condition(CondExpr::And(parts))
        }
    }

    fn apply_directive(
        &mut self,
        directive: CompilerDirective,
        source_path: &Path,
        warnings: &mut Vec<String>,
    ) {
        match directive {
            CompilerDirective::IfDef(symbol) => self.push_branch(CondExpr::Symbol(symbol)),
            CompilerDirective::IfNDef(symbol) => self.push_branch(normalize_condition(
                CondExpr::Not(Box::new(CondExpr::Symbol(symbol))),
            )),
            CompilerDirective::IfExpr(expr) => {
                let parsed = self.parse_if_expr("IF", &expr, source_path, warnings);
                self.push_branch(parsed);
            }
            CompilerDirective::IfOpt(option) => {
                self.push_branch(CondExpr::IfOpt(option));
            }
            CompilerDirective::ElseIfExpr(expr) => {
                let parsed = self.parse_if_expr("ELSEIF", &expr, source_path, warnings);
                self.enter_elseif(parsed, source_path, warnings);
            }
            CompilerDirective::Else => self.enter_else(source_path, warnings),
            CompilerDirective::EndIf => self.end_if(source_path, warnings),
            _ => {}
        }
    }

    fn note_unsupported_directive(
        &mut self,
        name: &str,
        source_path: &Path,
        warnings: &mut Vec<String>,
    ) {
        let upper = name.trim().to_ascii_uppercase();
        let warning_key = format!("{}|{}", source_path.display(), upper);
        if self.warned_unsupported.insert(warning_key) {
            warnings.push(format!(
                "warning: unsupported compiler directive {} in conditional uses context for {}",
                upper,
                source_path.display()
            ));
        }
        let unknown = CondExpr::Unknown(upper);
        if !self.sticky_unknowns.contains(&unknown) {
            self.sticky_unknowns.push(unknown);
        }
    }

    fn note_unsupported_expression(
        &mut self,
        kind: &str,
        expr: &str,
        source_path: &Path,
        warnings: &mut Vec<String>,
    ) {
        let rendered = expr.trim();
        let warning_key = format!(
            "{}|{}|{}",
            source_path.display(),
            kind.trim().to_ascii_uppercase(),
            rendered.to_ascii_uppercase()
        );
        if self.warned_unsupported.insert(warning_key) {
            warnings.push(format!(
                "warning: unsupported {} expression {} in conditional uses context for {}",
                kind.trim().to_ascii_uppercase(),
                rendered,
                source_path.display()
            ));
        }
    }

    fn parse_if_expr(
        &mut self,
        kind: &str,
        expr: &str,
        source_path: &Path,
        warnings: &mut Vec<String>,
    ) -> CondExpr {
        if let Some(parsed) = parse_if_expression(expr) {
            parsed
        } else {
            self.note_unsupported_expression(kind, expr, source_path, warnings);
            CondExpr::Unknown(format!(
                "{}: {}",
                kind.trim().to_ascii_uppercase(),
                expr.trim()
            ))
        }
    }

    fn push_branch(&mut self, condition: CondExpr) {
        let condition = normalize_condition(condition);
        self.frames.push(CondFrame {
            active_branch: condition.clone(),
            remaining_branch: other_not(condition),
            seen_else: false,
        });
    }

    fn enter_elseif(
        &mut self,
        condition: CondExpr,
        source_path: &Path,
        warnings: &mut Vec<String>,
    ) {
        let Some(frame) = self.frames.last_mut() else {
            warnings.push(format!(
                "warning: unmatched ELSEIF in {}",
                source_path.display()
            ));
            return;
        };
        if frame.seen_else {
            warnings.push(format!(
                "warning: ELSEIF after ELSE in {}",
                source_path.display()
            ));
            return;
        }

        let condition = normalize_condition(condition);
        let remaining = frame.remaining_branch.clone();
        frame.active_branch =
            normalize_condition(CondExpr::And(vec![remaining.clone(), condition.clone()]));
        frame.remaining_branch =
            normalize_condition(CondExpr::And(vec![remaining, other_not(condition)]));
    }

    fn enter_else(&mut self, source_path: &Path, warnings: &mut Vec<String>) {
        let Some(frame) = self.frames.last_mut() else {
            warnings.push(format!(
                "warning: unmatched ELSE in {}",
                source_path.display()
            ));
            return;
        };
        if frame.seen_else {
            warnings.push(format!(
                "warning: duplicate ELSE in {}",
                source_path.display()
            ));
            return;
        }

        frame.active_branch = frame.remaining_branch.clone();
        frame.remaining_branch = CondExpr::False;
        frame.seen_else = true;
    }

    fn end_if(&mut self, source_path: &Path, warnings: &mut Vec<String>) {
        if self.frames.pop().is_none() {
            warnings.push(format!(
                "warning: unmatched ENDIF in {}",
                source_path.display()
            ));
        }
    }
}

impl CondFrame {
    fn current_branch(&self) -> CondExpr {
        self.active_branch.clone()
    }
}

fn normalize_join(parts: Vec<CondExpr>, is_and: bool) -> CondExpr {
    let mut normalized = Vec::new();
    for part in parts {
        match normalize_condition(part) {
            CondExpr::And(nested) if is_and => normalized.extend(nested),
            CondExpr::Or(nested) if !is_and => normalized.extend(nested),
            CondExpr::True if is_and => {}
            CondExpr::False if !is_and => {}
            CondExpr::False if is_and => return CondExpr::False,
            CondExpr::True if !is_and => return CondExpr::True,
            other => normalized.push(other),
        }
    }

    normalized.sort();
    normalized.dedup();

    for part in &normalized {
        if normalized.contains(&other_not(part.clone())) {
            return if is_and {
                CondExpr::False
            } else {
                CondExpr::True
            };
        }
    }

    match normalized.len() {
        0 => {
            if is_and {
                CondExpr::True
            } else {
                CondExpr::False
            }
        }
        1 => normalized.into_iter().next().unwrap_or(CondExpr::True),
        _ => {
            if is_and {
                CondExpr::And(normalized)
            } else {
                CondExpr::Or(normalized)
            }
        }
    }
}

fn render_condition_with_precedence(expr: &CondExpr, parent_precedence: u8) -> String {
    let (precedence, rendered) = match expr {
        CondExpr::True => (3, "TRUE".to_string()),
        CondExpr::False => (3, "FALSE".to_string()),
        CondExpr::Symbol(symbol) => (3, symbol.to_ascii_uppercase()),
        CondExpr::IfOpt(option) => (3, format!("IFOPT({})", option.to_ascii_uppercase())),
        CondExpr::Unknown(label) => (3, format!("UNKNOWN({})", label.to_ascii_uppercase())),
        CondExpr::Not(inner) => (
            2,
            format!("NOT {}", render_condition_with_precedence(inner, 2)),
        ),
        CondExpr::And(parts) => (
            1,
            parts
                .iter()
                .map(|part| render_condition_with_precedence(part, 1))
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
        CondExpr::Or(parts) => (
            0,
            parts
                .iter()
                .map(|part| render_condition_with_precedence(part, 0))
                .collect::<Vec<_>>()
                .join(" OR "),
        ),
    };

    if precedence < parent_precedence {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn other_not(expr: CondExpr) -> CondExpr {
    normalize_condition(CondExpr::Not(Box::new(expr)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum IfExprToken {
    Ident(String),
    Number(String),
    LParen,
    RParen,
}

struct IfExprParser {
    tokens: Vec<IfExprToken>,
    pos: usize,
}

fn parse_if_expression(expr: &str) -> Option<CondExpr> {
    let tokens = tokenize_if_expression(expr)?;
    let mut parser = IfExprParser { tokens, pos: 0 };
    let parsed = parser.parse_or_expr()?;
    if parser.pos != parser.tokens.len() {
        return None;
    }
    Some(normalize_condition(parsed))
}

fn tokenize_if_expression(expr: &str) -> Option<Vec<IfExprToken>> {
    let bytes = expr.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'(' => {
                tokens.push(IfExprToken::LParen);
                i += 1;
            }
            b')' => {
                tokens.push(IfExprToken::RParen);
                i += 1;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
                {
                    i += 1;
                }
                tokens.push(IfExprToken::Ident(
                    String::from_utf8_lossy(&bytes[start..i]).to_string(),
                ));
            }
            byte if byte.is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                tokens.push(IfExprToken::Number(
                    String::from_utf8_lossy(&bytes[start..i]).to_string(),
                ));
            }
            _ => return None,
        }
    }

    Some(tokens)
}

impl IfExprParser {
    fn parse_or_expr(&mut self) -> Option<CondExpr> {
        let mut parts = vec![self.parse_and_expr()?];
        while self.consume_keyword("or") {
            parts.push(self.parse_and_expr()?);
        }
        Some(match parts.len() {
            1 => parts.pop().unwrap_or(CondExpr::False),
            _ => CondExpr::Or(parts),
        })
    }

    fn parse_and_expr(&mut self) -> Option<CondExpr> {
        let mut parts = vec![self.parse_not_expr()?];
        while self.consume_keyword("and") {
            parts.push(self.parse_not_expr()?);
        }
        Some(match parts.len() {
            1 => parts.pop().unwrap_or(CondExpr::False),
            _ => CondExpr::And(parts),
        })
    }

    fn parse_not_expr(&mut self) -> Option<CondExpr> {
        if self.consume_keyword("not") {
            return Some(CondExpr::Not(Box::new(self.parse_not_expr()?)));
        }
        self.parse_primary_expr()
    }

    fn parse_primary_expr(&mut self) -> Option<CondExpr> {
        match self.peek()?.clone() {
            IfExprToken::LParen => {
                self.pos += 1;
                let expr = self.parse_or_expr()?;
                if !matches!(self.peek(), Some(IfExprToken::RParen)) {
                    return None;
                }
                self.pos += 1;
                Some(expr)
            }
            IfExprToken::Ident(ident) if ident.eq_ignore_ascii_case("defined") => {
                self.pos += 1;
                self.parse_defined_expr()
            }
            IfExprToken::Ident(ident) if ident.eq_ignore_ascii_case("true") => {
                self.pos += 1;
                Some(CondExpr::True)
            }
            IfExprToken::Ident(ident) if ident.eq_ignore_ascii_case("false") => {
                self.pos += 1;
                Some(CondExpr::False)
            }
            IfExprToken::Ident(ident) => {
                self.pos += 1;
                Some(CondExpr::Symbol(ident))
            }
            IfExprToken::Number(number) => {
                self.pos += 1;
                match number.as_str() {
                    "1" => Some(CondExpr::True),
                    "0" => Some(CondExpr::False),
                    _ => None,
                }
            }
            IfExprToken::RParen => None,
        }
    }

    fn parse_defined_expr(&mut self) -> Option<CondExpr> {
        if matches!(self.peek(), Some(IfExprToken::LParen)) {
            self.pos += 1;
            let symbol = self.consume_ident()?;
            if !matches!(self.peek(), Some(IfExprToken::RParen)) {
                return None;
            }
            self.pos += 1;
            return Some(CondExpr::Symbol(symbol));
        }

        Some(CondExpr::Symbol(self.consume_ident()?))
    }

    fn consume_ident(&mut self) -> Option<String> {
        match self.peek()?.clone() {
            IfExprToken::Ident(ident) => {
                self.pos += 1;
                Some(ident)
            }
            _ => None,
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        match self.peek() {
            Some(IfExprToken::Ident(ident)) if ident.eq_ignore_ascii_case(keyword) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn peek(&self) -> Option<&IfExprToken> {
        self.tokens.get(self.pos)
    }
}

pub fn parse_unit_conditional_uses(
    path: &Path,
    bytes: &[u8],
    warnings: &mut Vec<String>,
) -> Vec<ConditionalUse> {
    let mut entries = Vec::new();
    let mut i = 0;
    let mut section = Section::None;
    let mut include_stack = vec![unit_cache::canonicalize_if_exists(path)];
    let mut condition_state = ConditionState::default();

    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'(' => {
                if let Some((directive, end)) = pas_lex::parse_compiler_directive(bytes, i) {
                    handle_scan_directive(directive, path, warnings, &mut condition_state);
                    i = end;
                    continue;
                }
                i = skip_non_directive_comment(bytes, i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            byte if pas_lex::is_ident_start(byte) => {
                let (token, next) = pas_lex::read_ident(bytes, i);
                if token.eq_ignore_ascii_case("interface") {
                    section = Section::Interface;
                } else if token.eq_ignore_ascii_case("implementation") {
                    section = Section::Implementation;
                } else if token.eq_ignore_ascii_case("uses") && section != Section::None {
                    let (next_i, _) = parse_uses_fragment(
                        path,
                        bytes,
                        next,
                        warnings,
                        &mut entries,
                        &mut include_stack,
                        &mut condition_state,
                    );
                    i = next_i;
                    continue;
                }
                i = next;
            }
            _ => i += 1,
        }
    }

    entries
}

pub fn parse_dpr_conditional_uses(
    path: &Path,
    bytes: &[u8],
    warnings: &mut Vec<String>,
) -> Option<Vec<ConditionalUse>> {
    let mut entries = Vec::new();
    let mut i = 0;
    let mut include_stack = vec![unit_cache::canonicalize_if_exists(path)];
    let mut condition_state = ConditionState::default();

    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'(' => {
                if let Some((directive, end)) = pas_lex::parse_compiler_directive(bytes, i) {
                    handle_scan_directive(directive, path, warnings, &mut condition_state);
                    i = end;
                    continue;
                }
                i = skip_non_directive_comment(bytes, i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            byte if pas_lex::is_ident_start(byte) => {
                let (token, next) = pas_lex::read_ident(bytes, i);
                if token.eq_ignore_ascii_case("uses") {
                    let (_, ended) = parse_uses_fragment(
                        path,
                        bytes,
                        next,
                        warnings,
                        &mut entries,
                        &mut include_stack,
                        &mut condition_state,
                    );
                    if ended && !entries.is_empty() {
                        return Some(entries);
                    }
                    break;
                }
                i = next;
            }
            _ => i += 1,
        }
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

pub fn collect_dpr_conditional_units(
    dpr_path: &Path,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    assumptions: &Assumptions,
    warnings: &mut Vec<String>,
) -> io::Result<Option<Vec<AggregatedConditionalUnit>>> {
    let bytes = fs::read(dpr_path)?;
    let Some(root_entries) = parse_dpr_conditional_uses(dpr_path, &bytes, warnings) else {
        return Ok(None);
    };

    let mut conditions_by_name: HashMap<String, (String, CondExpr)> = HashMap::new();
    let mut conditions_by_path: HashMap<PathBuf, (String, CondExpr)> = HashMap::new();
    let mut queue = VecDeque::new();

    for entry in root_entries {
        let root_condition = normalize_condition(entry.condition.clone());
        if evaluate_condition(&root_condition, assumptions) == EvalResult::Never {
            continue;
        }
        merge_name_condition(
            &mut conditions_by_name,
            entry.unit_name.clone(),
            root_condition.clone(),
        );

        if let Some(path) = resolve_conditional_use_path(
            dpr_path,
            dpr_path,
            &entry,
            project_cache,
            delphi_cache,
            warnings,
            true,
        ) {
            let canonical = unit_cache::canonicalize_if_exists(&path);
            if merge_path_condition(
                &mut conditions_by_path,
                canonical.clone(),
                entry.unit_name.clone(),
                root_condition,
            ) {
                queue.push_back(canonical);
            }
        }
    }

    while let Some(unit_path) = queue.pop_front() {
        let Some((_, current_condition)) = conditions_by_path.get(&unit_path).cloned() else {
            continue;
        };
        if evaluate_condition(&current_condition, assumptions) == EvalResult::Never {
            continue;
        }

        let conditional_uses =
            match load_conditional_uses(project_cache, delphi_cache, &unit_path, warnings)? {
                Some(uses) => uses,
                None => continue,
            };

        for dep in conditional_uses {
            let next_condition = normalize_condition(CondExpr::And(vec![
                current_condition.clone(),
                dep.condition.clone(),
            ]));
            if evaluate_condition(&next_condition, assumptions) == EvalResult::Never {
                continue;
            }

            merge_name_condition(
                &mut conditions_by_name,
                dep.unit_name.clone(),
                next_condition.clone(),
            );

            if let Some(dep_path) = resolve_conditional_use_path(
                &unit_path,
                &unit_path,
                &dep,
                project_cache,
                delphi_cache,
                warnings,
                false,
            ) {
                let canonical = unit_cache::canonicalize_if_exists(&dep_path);
                if merge_path_condition(
                    &mut conditions_by_path,
                    canonical.clone(),
                    dep.unit_name.clone(),
                    next_condition,
                ) {
                    queue.push_back(canonical);
                }
            }
        }
    }

    let mut units: Vec<AggregatedConditionalUnit> = conditions_by_name
        .into_values()
        .map(|(name, condition)| AggregatedConditionalUnit { name, condition })
        .collect();
    units.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(Some(units))
}

fn parse_uses_fragment(
    source_path: &Path,
    bytes: &[u8],
    mut i: usize,
    warnings: &mut Vec<String>,
    entries: &mut Vec<ConditionalUse>,
    include_stack: &mut Vec<PathBuf>,
    condition_state: &mut ConditionState,
) -> (usize, bool) {
    loop {
        let (next_i, include_ended) = skip_noise_and_includes(
            source_path,
            bytes,
            i,
            warnings,
            entries,
            include_stack,
            condition_state,
        );
        i = next_i;
        if include_ended {
            return (i, true);
        }
        if i >= bytes.len() {
            return (i, false);
        }
        if bytes[i] == b';' {
            return (i + 1, true);
        }
        if !pas_lex::is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }

        let condition = condition_state.current_condition();
        let (unit_name, next) = pas_lex::read_ident_with_dots(bytes, i);
        i = next;
        i = skip_noise_no_include(source_path, bytes, i, warnings, condition_state);

        let mut in_path = None;
        if let Some((token, next_token)) = peek_ident(bytes, i) {
            if token.eq_ignore_ascii_case("in") {
                i = next_token;
                i = skip_noise_no_include(source_path, bytes, i, warnings, condition_state);
                if let Some((path, end)) = pas_lex::read_string_literal(bytes, i) {
                    in_path = Some(path);
                    i = end;
                }
            }
        }

        let (next_i, delimiter, include_entries) = scan_to_delimiter(
            source_path,
            bytes,
            i,
            warnings,
            include_stack,
            condition_state,
        );
        entries.push(ConditionalUse {
            unit_name,
            in_path,
            condition,
        });
        entries.extend(include_entries);

        i = next_i;
        match delimiter {
            Some(b',') => i += 1,
            Some(b';') => return (i + 1, true),
            _ => return (i, false),
        }
    }
}

fn skip_noise_and_includes(
    source_path: &Path,
    bytes: &[u8],
    mut i: usize,
    warnings: &mut Vec<String>,
    entries: &mut Vec<ConditionalUse>,
    include_stack: &mut Vec<PathBuf>,
    condition_state: &mut ConditionState,
) -> (usize, bool) {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' | b'(' => {
                if let Some((directive, end)) = pas_lex::parse_compiler_directive(bytes, i) {
                    match directive {
                        CompilerDirective::Include(include_name) => {
                            let result = parse_include_entries(
                                include_name.as_str(),
                                source_path,
                                warnings,
                                include_stack,
                                condition_state,
                            );
                            if !result.entries.is_empty() {
                                entries.extend(result.entries);
                            }
                            i = end;
                            if result.ended {
                                return (i, true);
                            }
                            continue;
                        }
                        CompilerDirective::UnsupportedAffecting(name) => {
                            condition_state.note_unsupported_directive(
                                name.as_str(),
                                source_path,
                                warnings,
                            );
                            i = end;
                            continue;
                        }
                        CompilerDirective::Other(_) => {
                            i = end;
                            continue;
                        }
                        _ => {
                            condition_state.apply_directive(directive, source_path, warnings);
                            i = end;
                            continue;
                        }
                    }
                }
                i = skip_non_directive_comment(bytes, i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            _ => break,
        }
    }
    (i, false)
}

fn skip_noise_no_include(
    source_path: &Path,
    bytes: &[u8],
    mut i: usize,
    warnings: &mut Vec<String>,
    condition_state: &mut ConditionState,
) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' | b'(' => {
                if let Some((directive, end)) = pas_lex::parse_compiler_directive(bytes, i) {
                    match directive {
                        CompilerDirective::UnsupportedAffecting(name) => {
                            condition_state.note_unsupported_directive(
                                name.as_str(),
                                source_path,
                                warnings,
                            );
                            i = end;
                            continue;
                        }
                        CompilerDirective::Include(_) | CompilerDirective::Other(_) => {
                            i = end;
                            continue;
                        }
                        _ => {
                            condition_state.apply_directive(directive, source_path, warnings);
                            i = end;
                            continue;
                        }
                    }
                }
                i = skip_non_directive_comment(bytes, i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            _ => break,
        }
    }
    i
}

fn scan_to_delimiter(
    source_path: &Path,
    bytes: &[u8],
    mut i: usize,
    warnings: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
    condition_state: &mut ConditionState,
) -> (usize, Option<u8>, Vec<ConditionalUse>) {
    let mut include_entries = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b',' | b';' => return (i, Some(bytes[i]), include_entries),
            b'{' | b'(' => {
                if let Some((directive, end)) = pas_lex::parse_compiler_directive(bytes, i) {
                    match directive {
                        CompilerDirective::Include(include_name) => {
                            let result = parse_include_entries(
                                include_name.as_str(),
                                source_path,
                                warnings,
                                include_stack,
                                condition_state,
                            );
                            if !result.entries.is_empty() {
                                include_entries.extend(result.entries);
                            }
                            i = end;
                            if result.ended {
                                return (i, Some(b';'), include_entries);
                            }
                            continue;
                        }
                        CompilerDirective::UnsupportedAffecting(name) => {
                            condition_state.note_unsupported_directive(
                                name.as_str(),
                                source_path,
                                warnings,
                            );
                            i = end;
                            continue;
                        }
                        CompilerDirective::Other(_) => {
                            i = end;
                            continue;
                        }
                        _ => {
                            condition_state.apply_directive(directive, source_path, warnings);
                            i = end;
                            continue;
                        }
                    }
                }
                i = skip_non_directive_comment(bytes, i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            _ => i += 1,
        }
    }

    (i, None, include_entries)
}

fn parse_include_entries(
    include_name: &str,
    source_path: &Path,
    warnings: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
    condition_state: &mut ConditionState,
) -> IncludeParseResult {
    uses_include::with_include_bytes(
        include_name,
        source_path,
        warnings,
        include_stack,
        |include_path, bytes, warnings, include_stack| {
            let mut entries = Vec::new();
            let (_, ended) = parse_uses_fragment(
                include_path,
                bytes,
                0,
                warnings,
                &mut entries,
                include_stack,
                condition_state,
            );
            IncludeParseResult { entries, ended }
        },
    )
    .unwrap_or_default()
}

fn handle_scan_directive(
    directive: CompilerDirective,
    source_path: &Path,
    warnings: &mut Vec<String>,
    condition_state: &mut ConditionState,
) {
    match directive {
        CompilerDirective::IfDef(_)
        | CompilerDirective::IfNDef(_)
        | CompilerDirective::IfExpr(_)
        | CompilerDirective::IfOpt(_)
        | CompilerDirective::ElseIfExpr(_)
        | CompilerDirective::Else
        | CompilerDirective::EndIf => {
            condition_state.apply_directive(directive, source_path, warnings)
        }
        CompilerDirective::UnsupportedAffecting(name) => {
            condition_state.note_unsupported_directive(name.as_str(), source_path, warnings);
        }
        CompilerDirective::Include(_) | CompilerDirective::Other(_) => {}
    }
}

fn resolve_conditional_use_path(
    owner_path: &Path,
    source_path: &Path,
    conditional_use: &ConditionalUse,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    warnings: &mut Vec<String>,
    is_root: bool,
) -> Option<PathBuf> {
    if let Some(raw_path) = conditional_use.in_path.as_ref() {
        let resolved = resolve_relative_unit_path(source_path, raw_path);
        if resolved.is_file() {
            return Some(resolved);
        }

        warnings.push(format!(
            "warning: uses path not found for unit {} in {}: {}",
            conditional_use.unit_name,
            owner_path.display(),
            resolved.display()
        ));
    }

    match resolve_by_name(project_cache, delphi_cache, &conditional_use.unit_name) {
        ResolveByName::Unique { path, source } => {
            if conditional_use.in_path.is_none() && source == ResolutionSource::Project {
                let label = if is_root { "dpr" } else { "unit" };
                warnings.push(format!(
                    "warning: missing in-path for {} {} in {} (resolved via scan)",
                    label,
                    conditional_use.unit_name,
                    owner_path.display()
                ));
            }
            Some(path)
        }
        ResolveByName::Ambiguous { count, source } => {
            warnings.push(format!(
                "warning: ambiguous unit {} referenced by {} ({} {} matches)",
                conditional_use.unit_name,
                owner_path.display(),
                count,
                source_label(source)
            ));
            None
        }
        ResolveByName::NotFound => None,
    }
}

fn load_conditional_uses(
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    unit_path: &Path,
    warnings: &mut Vec<String>,
) -> io::Result<Option<Vec<ConditionalUse>>> {
    let canonical = unit_cache::canonicalize_if_exists(unit_path);
    if let Some(info) = project_cache.by_path.get(&canonical) {
        return Ok(Some(info.conditional_uses.clone()));
    }
    if let Some(delphi_cache) = delphi_cache {
        if let Some(info) = delphi_cache.by_path.get(&canonical) {
            return Ok(Some(info.conditional_uses.clone()));
        }
    }
    if !canonical.is_file() {
        warnings.push(format!(
            "warning: failed to read unit at {}",
            canonical.display()
        ));
        return Ok(None);
    }

    let bytes = fs::read(&canonical)?;
    Ok(Some(parse_unit_conditional_uses(
        &canonical, &bytes, warnings,
    )))
}

fn resolve_relative_unit_path(source_path: &Path, raw_path: &str) -> PathBuf {
    let candidate = PathBuf::from(raw_path);
    if candidate.is_absolute() {
        candidate
    } else {
        source_path
            .parent()
            .map(|parent| parent.join(&candidate))
            .unwrap_or(candidate)
    }
}

fn resolve_by_name(
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    unit_name: &str,
) -> ResolveByName {
    let key = unit_name.to_ascii_lowercase();
    if let Some(paths) = project_cache.by_name.get(&key) {
        if paths.len() > 1 {
            return ResolveByName::Ambiguous {
                count: paths.len(),
                source: ResolutionSource::Project,
            };
        }
        return ResolveByName::Unique {
            path: paths[0].clone(),
            source: ResolutionSource::Project,
        };
    }

    if let Some(delphi_cache) = delphi_cache {
        if let Some(paths) = delphi_cache.by_name.get(&key) {
            if paths.len() > 1 {
                return ResolveByName::Ambiguous {
                    count: paths.len(),
                    source: ResolutionSource::Delphi,
                };
            }
            return ResolveByName::Unique {
                path: paths[0].clone(),
                source: ResolutionSource::Delphi,
            };
        }
    }

    ResolveByName::NotFound
}

fn source_label(source: ResolutionSource) -> &'static str {
    match source {
        ResolutionSource::Project => "project",
        ResolutionSource::Delphi => "--delphi-path",
    }
}

fn merge_name_condition(
    conditions: &mut HashMap<String, (String, CondExpr)>,
    name: String,
    condition: CondExpr,
) {
    let key = name.to_ascii_lowercase();
    match conditions.get_mut(&key) {
        Some((_, existing)) => {
            *existing = normalize_condition(CondExpr::Or(vec![existing.clone(), condition]));
        }
        None => {
            conditions.insert(key, (name, condition));
        }
    }
}

fn merge_path_condition(
    conditions: &mut HashMap<PathBuf, (String, CondExpr)>,
    path: PathBuf,
    name: String,
    condition: CondExpr,
) -> bool {
    match conditions.get_mut(&path) {
        Some((_, existing)) => {
            let merged = normalize_condition(CondExpr::Or(vec![existing.clone(), condition]));
            if *existing == merged {
                false
            } else {
                *existing = merged;
                true
            }
        }
        None => {
            conditions.insert(path, (name, condition));
            true
        }
    }
}

fn peek_ident(bytes: &[u8], i: usize) -> Option<(String, usize)> {
    if i < bytes.len() && pas_lex::is_ident_start(bytes[i]) {
        let (token, next) = pas_lex::read_ident(bytes, i);
        return Some((token, next));
    }
    None
}

fn skip_non_directive_comment(bytes: &[u8], i: usize) -> usize {
    if bytes.get(i) == Some(&b'{') {
        pas_lex::skip_brace_comment(bytes, i + 1)
    } else if bytes.get(i) == Some(&b'(') && bytes.get(i + 1) == Some(&b'*') {
        pas_lex::skip_paren_comment(bytes, i + 2)
    } else {
        i + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_condition_flattens_and_dedupes() {
        let expr = CondExpr::And(vec![
            CondExpr::Symbol("debug".to_string()),
            CondExpr::And(vec![
                CondExpr::Symbol("DEBUG".to_string()),
                CondExpr::Not(Box::new(CondExpr::Symbol("trace".to_string()))),
            ]),
        ]);

        assert_eq!(
            normalize_condition(expr),
            CondExpr::And(vec![
                CondExpr::Symbol("DEBUG".to_string()),
                CondExpr::Not(Box::new(CondExpr::Symbol("TRACE".to_string()))),
            ])
        );
    }

    #[test]
    fn evaluate_condition_supports_unknown_symbols() {
        let expr = normalize_condition(CondExpr::And(vec![
            CondExpr::Symbol("DEBUG".to_string()),
            CondExpr::Not(Box::new(CondExpr::Symbol("TRACE".to_string()))),
        ]));

        let mut assumptions = Assumptions::default();
        assumptions.set("debug", AssumedValue::On);
        assert_eq!(evaluate_condition(&expr, &assumptions), EvalResult::Maybe);

        assumptions.set("trace", AssumedValue::Off);
        assert_eq!(evaluate_condition(&expr, &assumptions), EvalResult::Always);

        assumptions.set("trace", AssumedValue::On);
        assert_eq!(evaluate_condition(&expr, &assumptions), EvalResult::Never);
    }

    #[test]
    fn parse_unit_conditional_uses_tracks_nested_conditions() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let src = br#"
unit Demo;
interface
{$IFDEF DEBUG}
uses Foo,
  {$IFNDEF TRACE} Bar, {$ENDIF}
  {$ELSE}
  Baz;
{$ENDIF}
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].unit_name, "Foo");
        assert_eq!(render_condition(&entries[0].condition), "DEBUG");
        assert_eq!(entries[1].unit_name, "Bar");
        assert_eq!(
            render_condition(&entries[1].condition),
            "DEBUG AND NOT TRACE"
        );
        assert_eq!(entries[2].unit_name, "Baz");
        assert_eq!(render_condition(&entries[2].condition), "NOT DEBUG");
    }

    #[test]
    fn parse_dpr_conditional_uses_tracks_root_conditions() {
        let root = temp_dir();
        let dpr_path = root.join("App.dpr");
        let src = br#"
program App;
{$IFDEF DEBUG}
uses
  DebugUnit in 'DebugUnit.pas',
{$ELSE}
  ReleaseUnit in 'ReleaseUnit.pas';
{$ENDIF}
begin
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_dpr_conditional_uses(&dpr_path, src, &mut warnings).expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(render_condition(&entries[0].condition), "DEBUG");
        assert_eq!(render_condition(&entries[1].condition), "NOT DEBUG");
    }

    #[test]
    fn parse_unit_conditional_uses_supports_if_boolean_expressions() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let src = br#"
unit Demo;
interface
{$IF defined(DEBUG) and not defined(TRACE)}
uses Foo;
{$ENDIF}
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            render_condition(&entries[0].condition),
            "DEBUG AND NOT TRACE"
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn parse_unit_conditional_uses_supports_elseif_chains() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let src = br#"
unit Demo;
interface
{$IF defined(A)}
uses Foo,
{$ELSEIF defined(B)}
  Bar,
{$ELSE}
  Baz;
{$ENDIF}
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 3);
        assert_eq!(render_condition(&entries[0].condition), "A");
        assert_eq!(render_condition(&entries[1].condition), "B AND NOT A");
        assert_eq!(render_condition(&entries[2].condition), "NOT A AND NOT B");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn parse_unit_conditional_uses_supports_ifopt_directives() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let src = br#"
unit Demo;
interface
{$IFOPT N+}
uses Foo,
{$ELSE}
  Bar;
{$ENDIF}
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 2);
        assert_eq!(render_condition(&entries[0].condition), "IFOPT(N+)");
        assert_eq!(render_condition(&entries[1].condition), "NOT IFOPT(N+)");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn parse_unit_conditional_uses_keeps_unsupported_if_expressions_branch_local() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let src = br#"
unit Demo;
interface
{$IF RTLVersion >= 14}
uses Foo,
{$ELSE}
  Bar;
{$ENDIF}
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            render_condition(&entries[0].condition),
            "UNKNOWN(IF: RTLVERSION >= 14)"
        );
        assert_eq!(
            render_condition(&entries[1].condition),
            "NOT UNKNOWN(IF: RTLVERSION >= 14)"
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unsupported IF expression RTLVersion >= 14"));
    }

    #[test]
    fn parse_unit_conditional_uses_supports_include_fragments() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let include_path = root.join("Uses.inc");
        fs::write(
            &include_path,
            "{$IFDEF DEBUG} Foo, {$ENDIF}\n{$IFNDEF TRACE} Bar, {$ENDIF}",
        )
        .unwrap();
        let src = br#"
unit Demo;
interface
uses {$I Uses.inc} Baz;
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 3);
        assert_eq!(render_condition(&entries[0].condition), "DEBUG");
        assert_eq!(render_condition(&entries[1].condition), "NOT TRACE");
        assert_eq!(render_condition(&entries[2].condition), "TRUE");
    }

    #[test]
    fn parse_unit_conditional_uses_marks_unsupported_directives_unknown() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let src = br#"
unit Demo;
interface
{$DEFINE EXPERIMENTAL}
uses Foo;
implementation
end.
"#;

        let mut warnings = Vec::new();
        let entries = parse_unit_conditional_uses(&unit_path, src, &mut warnings);
        assert_eq!(entries.len(), 1);
        assert_eq!(render_condition(&entries[0].condition), "UNKNOWN(DEFINE)");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unsupported compiler directive DEFINE"));
    }

    #[test]
    fn bucket_conditionals_groups_simple_and_complex_conditions() {
        let buckets = bucket_conditionals(&[
            AggregatedConditionalUnit {
                name: "Foo".to_string(),
                condition: CondExpr::True,
            },
            AggregatedConditionalUnit {
                name: "Bar".to_string(),
                condition: CondExpr::Symbol("debug".to_string()),
            },
            AggregatedConditionalUnit {
                name: "Baz".to_string(),
                condition: CondExpr::Not(Box::new(CondExpr::Symbol("trace".to_string()))),
            },
            AggregatedConditionalUnit {
                name: "Qux".to_string(),
                condition: normalize_condition(CondExpr::And(vec![
                    CondExpr::Symbol("debug".to_string()),
                    CondExpr::Not(Box::new(CondExpr::Symbol("trace".to_string()))),
                ])),
            },
        ]);

        assert_eq!(buckets.unconditional, vec!["Foo"]);
        assert_eq!(
            buckets.positive,
            vec![("DEBUG".to_string(), vec!["Bar".to_string()])]
        );
        assert_eq!(
            buckets.negative,
            vec![("TRACE".to_string(), vec!["Baz".to_string()])]
        );
        assert_eq!(
            buckets.complex,
            vec![("Qux".to_string(), "DEBUG AND NOT TRACE".to_string())]
        );
    }

    fn temp_dir() -> PathBuf {
        let mut root = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("fixdpr_conditionals_{nanos}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }
}
