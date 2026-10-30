use crate::error::{Error, ErrorCode};
use crate::span::Span;
use crate::syntax::{Token, TokenKind, Trivia, TriviaKind};

/// The eleven characters that never appear inside a word.
pub(crate) const SPECIAL: [char; 11] = [',', ':', '.', '(', ')', '[', ']', '{', '}', '#', '"'];

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `alnum` in the grammar: a letter, an ASCII digit or `_`.
fn is_alnum(c: char) -> bool {
    c.is_alphabetic() || c.is_ascii_digit() || c == '_'
}

fn is_symbol(c: char) -> bool {
    !c.is_whitespace() && !SPECIAL.contains(&c) && !is_alnum(c)
}

/// All tokens of `source`, ending with [`TokenKind::End`], and the string errors.
pub(crate) fn lex(source: &str) -> (Vec<Token>, Vec<Error>) {
    let mut lexer = Lexer {
        source,
        position: 0,
        errors: Vec::new(),
    };
    let mut tokens = Vec::new();
    loop {
        let leading = lexer.trivia(false);
        let token = lexer.token(leading);
        let done = token.kind == TokenKind::End;
        tokens.push(token);
        if done {
            return (tokens, lexer.errors);
        }
    }
}

struct Lexer<'a> {
    source: &'a str,
    position: usize,
    errors: Vec<Error>,
}

impl Lexer<'_> {
    fn rest(&self) -> &str {
        &self.source[self.position..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.rest().chars().nth(1)
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.position += c.len_utf8();
        }
    }

    fn bump_while(&mut self, keep: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&keep) {
            self.bump();
        }
    }

    fn text(&self, start: usize) -> String {
        self.source[start..self.position].to_string()
    }

    /// Whitespace and comments. With `same_line`, stops before a newline.
    fn trivia(&mut self, same_line: bool) -> Vec<Trivia> {
        let mut trivia = Vec::new();
        loop {
            let start = self.position;
            let kind = match self.peek() {
                Some('#') => {
                    self.bump_while(|c| c != '\n');
                    TriviaKind::Comment
                }
                Some(c) if c.is_whitespace() && !(same_line && c == '\n') => {
                    self.bump_while(|c| c.is_whitespace() && !(same_line && c == '\n'));
                    TriviaKind::Whitespace
                }
                _ => return trivia,
            };
            trivia.push(Trivia {
                kind,
                text: self.text(start),
                span: Span::new(start, self.position),
            });
        }
    }

    fn token(&mut self, leading: Vec<Trivia>) -> Token {
        let start = self.position;
        let kind = match self.peek() {
            None => TokenKind::End,
            Some(c) => {
                self.bump();
                match c {
                    ',' => TokenKind::Comma,
                    ':' => TokenKind::Colon,
                    '.' => TokenKind::Dot,
                    '(' => TokenKind::OpenParen,
                    ')' => TokenKind::CloseParen,
                    '[' => TokenKind::OpenBracket,
                    ']' => TokenKind::CloseBracket,
                    '{' => TokenKind::OpenBrace,
                    '}' => TokenKind::CloseBrace,
                    '"' => self.string(start),
                    c if c.is_ascii_digit() => {
                        self.number();
                        TokenKind::Number
                    }
                    c if is_name_start(c) => {
                        self.bump_while(is_name_continue);
                        TokenKind::Word
                    }
                    _ => {
                        self.bump_while(is_symbol);
                        TokenKind::Word
                    }
                }
            }
        };
        let span = Span::new(start, self.position);
        let text = self.text(start);
        let trailing = self.trivia(true);
        Token {
            kind,
            text,
            span,
            leading,
            trailing,
        }
    }

    /// The rest of a number: alnum characters, and `.` only before an ASCII digit.
    fn number(&mut self) {
        loop {
            match self.peek() {
                Some(c) if is_alnum(c) => self.bump(),
                Some('.') if self.peek_second().is_some_and(|c| c.is_ascii_digit()) => {
                    self.bump();
                    self.bump();
                }
                _ => return,
            }
        }
    }

    /// The rest of a string after its opening `"`.
    fn string(&mut self, start: usize) -> TokenKind {
        let mut value = String::new();
        loop {
            match self.peek() {
                None => {
                    self.errors.push(Error::new(
                        ErrorCode::UnterminatedString,
                        Span::new(start, self.position),
                        "found a string that runs to the end of input; expected a closing `\"`; add `\"` where the string ends",
                    ));
                    break;
                }
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    let escape_start = self.position;
                    self.bump();
                    self.escape(escape_start, &mut value);
                }
                Some(c) => {
                    self.bump();
                    value.push(c);
                }
            }
        }
        TokenKind::String { value }
    }

    /// The rest of an escape after its `\`. Escape letters are case-insensitive.
    /// An invalid escape keeps its source text in the decoded value.
    fn escape(&mut self, start: usize, value: &mut String) {
        let Some(c) = self.peek() else {
            // The string is unterminated; `string` reports it.
            value.push('\\');
            return;
        };
        self.bump();
        let decoded = match c.to_ascii_lowercase() {
            '"' => '"',
            '\\' => '\\',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'u' => return self.unicode_escape(start, value),
            _ => {
                let found = self.text(start);
                self.invalid_escape(
                    start,
                    format!(
                        "found the unknown escape `{found}` in a string; expected one of `\\\"` `\\\\` `\\n` `\\t` `\\r` `\\u{{…}}`; write `\\\\` for a literal backslash"
                    ),
                );
                value.push_str(&found);
                return;
            }
        };
        value.push(decoded);
    }

    /// The rest of `\u{…}` after its `u`: 1 to 6 hex digits naming a Unicode scalar value.
    fn unicode_escape(&mut self, start: usize, value: &mut String) {
        if self.peek() != Some('{') {
            let found = self.text(start);
            self.invalid_escape(
                start,
                format!(
                    "found `{found}` without `{{` in a string; expected `\\u{{` then 1 to 6 hex digits then `}}`; write the code point as `\\u{{41}}`"
                ),
            );
            value.push_str(&found);
            return;
        }
        self.bump();
        let digits_start = self.position;
        self.bump_while(|c| c.is_ascii_hexdigit());
        let digits = self.text(digits_start);
        let closed = self.peek() == Some('}');
        if closed {
            self.bump();
        }
        let found = self.text(start);
        let scalar = u32::from_str_radix(&digits, 16)
            .ok()
            .and_then(char::from_u32);
        let problem = match (closed, digits.len(), scalar) {
            (false, _, _) => "has no closing `}`",
            (true, 0, _) => "has no hex digits",
            (true, 7.., _) => "has more than 6 hex digits",
            (true, _, None) => "is not a Unicode scalar value",
            (true, _, Some(c)) => {
                value.push(c);
                return;
            }
        };
        self.invalid_escape(
            start,
            format!(
                "found `{found}` in a string, which {problem}; expected `\\u{{` then 1 to 6 hex digits naming a Unicode scalar value (at most 10FFFF, not D800 to DFFF) then `}}`; fix the code point"
            ),
        );
        value.push_str(&found);
    }

    fn invalid_escape(&mut self, start: usize, message: String) {
        self.errors.push(Error::new(
            ErrorCode::InvalidEscape,
            Span::new(start, self.position),
            message,
        ));
    }
}
