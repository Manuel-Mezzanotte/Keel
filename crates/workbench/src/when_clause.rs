//! VS Code `when` clauses, such as `vsauto.running && editorTextFocus`,
//! which decide whether a key binding applies.
//!
//! Supported: keys, `!`, `&&`, `||`, parentheses, `==`, `!=`, `===`, `!==`,
//! `=~` with a regular expression written as `/.../`, `<`, `<=`, `>`, `>=`,
//! and string, number and boolean values.

use std::collections::HashMap;

use serde_json::Value;

/// Values of context keys, set by Keel and by extensions with `setContext`.
#[derive(Clone, Debug, Default)]
pub struct Context {
    values: HashMap<String, Value>,
}

impl Context {
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn remove(&mut self, key: &str) {
        self.values.remove(key);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
}

/// A parsed clause, ready to be evaluated many times.
#[derive(Clone, Debug, PartialEq)]
pub enum WhenClause {
    True,
    Key(String),
    Not(Box<WhenClause>),
    And(Vec<WhenClause>),
    Or(Vec<WhenClause>),
    Compare(String, Comparison, Literal),
    Matches(String, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Text(String),
    Number(f64),
    Bool(bool),
}

impl WhenClause {
    /// Parses a clause. An empty clause always applies.
    pub fn parse(source: &str) -> Result<Self, String> {
        let tokens = tokenize(source)?;
        if tokens.is_empty() {
            return Ok(WhenClause::True);
        }
        let mut parser = Parser {
            tokens,
            position: 0,
        };
        let clause = parser.or()?;
        if parser.position != parser.tokens.len() {
            return Err(format!("unexpected text in when clause: {source}"));
        }
        Ok(clause)
    }

    pub fn evaluate(&self, context: &Context) -> bool {
        match self {
            WhenClause::True => true,
            WhenClause::Key(key) => context.get(key).is_some_and(truthy),
            WhenClause::Not(inner) => !inner.evaluate(context),
            WhenClause::And(parts) => parts.iter().all(|part| part.evaluate(context)),
            WhenClause::Or(parts) => parts.iter().any(|part| part.evaluate(context)),
            WhenClause::Compare(key, comparison, literal) => {
                compare(context.get(key), *comparison, literal)
            }
            WhenClause::Matches(key, pattern) => {
                let value = context.get(key).map(as_text).unwrap_or_default();
                simple_regex_match(pattern, &value)
            }
        }
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn as_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn compare(value: Option<&Value>, comparison: Comparison, literal: &Literal) -> bool {
    let equal = match (value, literal) {
        (None | Some(Value::Null), Literal::Bool(expected)) => !expected,
        (None | Some(Value::Null), _) => false,
        (Some(Value::Bool(actual)), Literal::Bool(expected)) => actual == expected,
        (Some(actual), Literal::Number(expected)) => {
            actual.as_f64().or_else(|| as_text(actual).parse().ok()) == Some(*expected)
        }
        (Some(actual), Literal::Text(expected)) => as_text(actual) == *expected,
        (Some(actual), Literal::Bool(expected)) => as_text(actual) == expected.to_string(),
    };
    let ordered = |check: fn(f64, f64) -> bool| {
        let actual = value.and_then(|value| value.as_f64().or_else(|| as_text(value).parse().ok()));
        match (actual, literal) {
            (Some(actual), Literal::Number(expected)) => check(actual, *expected),
            _ => false,
        }
    };
    match comparison {
        Comparison::Equal => equal,
        Comparison::NotEqual => !equal,
        Comparison::Less => ordered(|a, b| a < b),
        Comparison::LessOrEqual => ordered(|a, b| a <= b),
        Comparison::Greater => ordered(|a, b| a > b),
        Comparison::GreaterOrEqual => ordered(|a, b| a >= b),
    }
}

/// Matches the small regular expressions `when` clauses use: literal text
/// with `^` and `$` anchors and a trailing `i` flag. Anything fancier never
/// matches, which keeps the binding off rather than wrongly on.
fn simple_regex_match(pattern: &str, value: &str) -> bool {
    let (body, insensitive) = match pattern.strip_suffix('i') {
        Some(body) if body.ends_with('/') => (body, true),
        _ => (pattern, false),
    };
    let Some(body) = body
        .strip_prefix('/')
        .and_then(|body| body.strip_suffix('/'))
    else {
        return false;
    };
    let starts = body.starts_with('^');
    let ends = body.ends_with('$') && !body.ends_with("\\$");
    let core = body.trim_start_matches('^');
    let core = if ends { &core[..core.len() - 1] } else { core };
    if core
        .chars()
        .any(|character| "[](){}*+?|".contains(character))
    {
        return false;
    }
    let core = core.replace('\\', "");
    let (core, value) = if insensitive {
        (core.to_lowercase(), value.to_lowercase())
    } else {
        (core, value.to_owned())
    };
    match (starts, ends) {
        (true, true) => value == core,
        (true, false) => value.starts_with(&core),
        (false, true) => value.ends_with(&core),
        (false, false) => value.contains(&core),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Regex(String),
    Not,
    And,
    Or,
    Open,
    Close,
    Operator(&'static str),
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        let rest: String = characters[index..characters.len().min(index + 3)]
            .iter()
            .collect();
        match character {
            ' ' | '\t' | '\n' => index += 1,
            '(' => {
                tokens.push(Token::Open);
                index += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                index += 1;
            }
            '&' if next == Some('&') => {
                tokens.push(Token::And);
                index += 2;
            }
            '|' if next == Some('|') => {
                tokens.push(Token::Or);
                index += 2;
            }
            '=' | '!' if rest.starts_with("===") || rest.starts_with("!==") => {
                tokens.push(Token::Operator(if character == '=' { "==" } else { "!=" }));
                index += 3;
            }
            '=' if next == Some('=') => {
                tokens.push(Token::Operator("=="));
                index += 2;
            }
            '!' if next == Some('=') => {
                tokens.push(Token::Operator("!="));
                index += 2;
            }
            '=' if next == Some('~') => {
                tokens.push(Token::Operator("=~"));
                index += 2;
            }
            '<' | '>' => {
                let with_equal = next == Some('=');
                tokens.push(Token::Operator(match (character, with_equal) {
                    ('<', true) => "<=",
                    ('<', false) => "<",
                    ('>', true) => ">=",
                    _ => ">",
                }));
                index += if with_equal { 2 } else { 1 };
            }
            '!' => {
                tokens.push(Token::Not);
                index += 1;
            }
            '\'' | '"' => {
                let quote = character;
                let mut text = String::new();
                index += 1;
                while index < characters.len() && characters[index] != quote {
                    text.push(characters[index]);
                    index += 1;
                }
                if index == characters.len() {
                    return Err(format!("unclosed string in when clause: {source}"));
                }
                index += 1;
                tokens.push(Token::Text(text));
            }
            '/' if matches!(tokens.last(), Some(Token::Operator("=~"))) => {
                let mut regex = String::from("/");
                index += 1;
                while index < characters.len() && characters[index] != '/' {
                    if characters[index] == '\\' && index + 1 < characters.len() {
                        regex.push(characters[index]);
                        index += 1;
                    }
                    regex.push(characters[index]);
                    index += 1;
                }
                regex.push('/');
                index += 1;
                while index < characters.len() && characters[index].is_ascii_alphabetic() {
                    regex.push(characters[index]);
                    index += 1;
                }
                tokens.push(Token::Regex(regex));
            }
            _ => {
                let mut word = String::new();
                while index < characters.len() && !" \t\n()&|=!<>'\"".contains(characters[index]) {
                    word.push(characters[index]);
                    index += 1;
                }
                if word.is_empty() {
                    return Err(format!(
                        "unexpected character '{character}' in when clause: {source}"
                    ));
                }
                tokens.push(Token::Word(word));
            }
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += 1;
        token
    }

    fn or(&mut self) -> Result<WhenClause, String> {
        let mut parts = vec![self.and()?];
        while self.peek() == Some(&Token::Or) {
            self.position += 1;
            parts.push(self.and()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            WhenClause::Or(parts)
        })
    }

    fn and(&mut self) -> Result<WhenClause, String> {
        let mut parts = vec![self.unary()?];
        while self.peek() == Some(&Token::And) {
            self.position += 1;
            parts.push(self.unary()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            WhenClause::And(parts)
        })
    }

    fn unary(&mut self) -> Result<WhenClause, String> {
        if self.peek() == Some(&Token::Not) {
            self.position += 1;
            return Ok(WhenClause::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<WhenClause, String> {
        match self.next() {
            Some(Token::Open) => {
                let inner = self.or()?;
                match self.next() {
                    Some(Token::Close) => Ok(inner),
                    _ => Err("missing ')' in when clause".to_owned()),
                }
            }
            Some(Token::Word(word)) if word == "true" => Ok(WhenClause::True),
            Some(Token::Word(word)) if word == "false" => {
                Ok(WhenClause::Not(Box::new(WhenClause::True)))
            }
            Some(Token::Word(key)) => self.term(key),
            other => Err(format!("unexpected {other:?} in when clause")),
        }
    }

    fn term(&mut self, key: String) -> Result<WhenClause, String> {
        let Some(Token::Operator(operator)) = self.peek().cloned() else {
            return Ok(WhenClause::Key(key));
        };
        self.position += 1;
        if operator == "=~" {
            return match self.next() {
                Some(Token::Regex(pattern)) => Ok(WhenClause::Matches(key, pattern)),
                other => Err(format!("expected a regular expression, found {other:?}")),
            };
        }
        let literal = match self.next() {
            Some(Token::Text(text)) => Literal::Text(text),
            Some(Token::Word(word)) => match word.as_str() {
                "true" => Literal::Bool(true),
                "false" => Literal::Bool(false),
                _ => word
                    .parse()
                    .map(Literal::Number)
                    .unwrap_or(Literal::Text(word)),
            },
            other => return Err(format!("expected a value, found {other:?}")),
        };
        let comparison = match operator {
            "==" => Comparison::Equal,
            "!=" => Comparison::NotEqual,
            "<" => Comparison::Less,
            "<=" => Comparison::LessOrEqual,
            ">" => Comparison::Greater,
            _ => Comparison::GreaterOrEqual,
        };
        Ok(WhenClause::Compare(key, comparison, literal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        let mut context = Context::default();
        context.set("vsauto.running", true);
        context.set("vsauto.ghostFocused", true);
        context.set("editorTextFocus", true);
        context.set("suggestWidgetVisible", false);
        context.set("resourceExtname", ".tsx");
        context.set("editorLangId", "typescriptreact");
        context.set("count", 3);
        context
    }

    fn check(source: &str) -> bool {
        WhenClause::parse(source).unwrap().evaluate(&context())
    }

    #[test]
    fn the_clauses_vsauto_ghost_uses() {
        assert!(check("vsauto.running"));
        assert!(check(
            "vsauto.running && vsauto.ghostFocused && editorTextFocus"
        ));
        assert!(check(
            "vsauto.running && vsauto.ghostFocused && editorTextFocus && !suggestWidgetVisible"
        ));
        assert!(!check(
            "vsauto.running && vsauto.ghostFocused && editorTextFocus && suggestWidgetVisible"
        ));
    }

    #[test]
    fn missing_keys_are_false() {
        assert!(!check("somethingElse"));
        assert!(check("!somethingElse"));
        assert!(check("somethingElse == false"));
    }

    #[test]
    fn or_parentheses_and_precedence() {
        assert!(check("suggestWidgetVisible || editorTextFocus"));
        assert!(!check(
            "!editorTextFocus || suggestWidgetVisible && vsauto.running"
        ));
        assert!(check(
            "(suggestWidgetVisible || vsauto.running) && editorTextFocus"
        ));
        assert!(check(""));
    }

    #[test]
    fn comparisons() {
        assert!(check("editorLangId == typescriptreact"));
        assert!(check("editorLangId == 'typescriptreact'"));
        assert!(check("editorLangId === \"typescriptreact\""));
        assert!(check("resourceExtname != .ts"));
        assert!(check("count > 2 && count <= 3"));
        assert!(!check("count < 3"));
        assert!(check("vsauto.running == true"));
    }

    #[test]
    fn simple_regular_expressions() {
        assert!(check("resourceExtname =~ /\\.tsx$/"));
        assert!(check("editorLangId =~ /^TYPESCRIPT/i"));
        assert!(!check("editorLangId =~ /^javascript/"));
    }

    #[test]
    fn broken_clauses_are_errors() {
        assert!(WhenClause::parse("(a && b").is_err());
        assert!(WhenClause::parse("a == ").is_err());
        assert!(WhenClause::parse("a 'b'").is_err());
    }
}
