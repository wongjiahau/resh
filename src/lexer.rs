use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub span: Span,
    pub msg: String,
}

impl Diag {
    pub fn new(span: Span, msg: impl Into<String>) -> Self {
        Diag {
            span,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.msg)
    }
}

pub type Res<T> = Result<T, Diag>;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Word(String),
    Kw(String),
    Int(i64),
    Float(f64),
    Str(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Question,
    Arrow,
    Pipe,
    Eq,
    Lt,
    Gt,
    Underscore,
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub const KEYWORDS: &[&str] = &[
    "FUNCTION", "RECORD", "ENUM", "MATCH", "LET", "IF", "THEN", "ELSE", "NATIVE",
];

struct Lx {
    c: Vec<char>,
    i: usize,
    line: usize,
    col: usize,
}

impl Lx {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn peek2(&self) -> Option<char> {
        self.c.get(self.i + 1).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let ch = self.c.get(self.i).copied()?;
        self.i += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }
}

fn word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '\''
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\'' || c == '-'
}

pub fn lex(src: &str) -> Res<Vec<Token>> {
    let mut lx = Lx {
        c: src.chars().collect(),
        i: 0,
        line: 1,
        col: 1,
    };
    let mut out = Vec::new();
    while let Some(c) = lx.peek() {
        let span = Span {
            line: lx.line,
            col: lx.col,
        };
        let mut push = |tok: Tok| out.push(Token { tok, span });
        match c {
            ' ' | '\t' | '\r' => {
                lx.bump();
            }
            '\n' | ';' => {
                lx.bump();
                push(Tok::Newline);
            }
            '/' if lx.peek2() == Some('/') => {
                while let Some(ch) = lx.peek() {
                    if ch == '\n' {
                        break;
                    }
                    lx.bump();
                }
            }
            '(' => {
                lx.bump();
                push(Tok::LParen);
            }
            ')' => {
                lx.bump();
                push(Tok::RParen);
            }
            '{' => {
                lx.bump();
                push(Tok::LBrace);
            }
            '}' => {
                lx.bump();
                push(Tok::RBrace);
            }
            '[' => {
                lx.bump();
                push(Tok::LBracket);
            }
            ']' => {
                lx.bump();
                push(Tok::RBracket);
            }
            ',' => {
                lx.bump();
                push(Tok::Comma);
            }
            ':' => {
                lx.bump();
                push(Tok::Colon);
            }
            '?' => {
                lx.bump();
                push(Tok::Question);
            }
            '<' => {
                lx.bump();
                push(Tok::Lt);
            }
            '>' => {
                lx.bump();
                push(Tok::Gt);
            }
            '=' => {
                lx.bump();
                if lx.peek() == Some('>') {
                    lx.bump();
                    push(Tok::Arrow);
                } else {
                    push(Tok::Eq);
                }
            }
            '|' => {
                lx.bump();
                if lx.peek() == Some('>') {
                    lx.bump();
                    push(Tok::Pipe);
                } else {
                    return Err(Diag::new(
                        span,
                        "unexpected `|`; did you mean the pipe `|>`?",
                    ));
                }
            }
            '"' => {
                lx.bump();
                let mut s = String::new();
                loop {
                    match lx.bump() {
                        None => return Err(Diag::new(span, "unterminated string literal")),
                        Some('"') => break,
                        Some('\\') => match lx.bump() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            other => {
                                return Err(Diag::new(
                                    span,
                                    format!(
                                        "unknown escape `\\{}` in string",
                                        other.unwrap_or(' ')
                                    ),
                                ));
                            }
                        },
                        Some(ch) => s.push(ch),
                    }
                }
                push(Tok::Str(s));
            }
            d if d.is_ascii_digit() => {
                let mut s = String::new();
                while let Some(ch) = lx.peek() {
                    if ch.is_ascii_digit() {
                        s.push(ch);
                        lx.bump();
                    } else {
                        break;
                    }
                }
                if lx.peek() == Some('.') && lx.peek2().is_some_and(|x| x.is_ascii_digit()) {
                    s.push('.');
                    lx.bump();
                    while let Some(ch) = lx.peek() {
                        if ch.is_ascii_digit() {
                            s.push(ch);
                            lx.bump();
                        } else {
                            break;
                        }
                    }
                    let f = s.parse().map_err(|_| Diag::new(span, "invalid number"))?;
                    push(Tok::Float(f));
                } else {
                    let n = s
                        .parse()
                        .map_err(|_| Diag::new(span, "integer literal out of range"))?;
                    push(Tok::Int(n));
                }
            }
            w if word_start(w) => {
                let mut s = String::new();
                while let Some(ch) = lx.peek() {
                    if word_char(ch) {
                        s.push(ch);
                        lx.bump();
                    } else {
                        break;
                    }
                }
                if s == "_" {
                    push(Tok::Underscore);
                } else if KEYWORDS.contains(&s.as_str()) {
                    push(Tok::Kw(s));
                } else {
                    push(Tok::Word(s));
                }
            }
            other => {
                return Err(Diag::new(span, format!("unexpected character `{other}`")));
            }
        }
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span {
            line: lx.line,
            col: lx.col,
        },
    });
    Ok(out)
}
