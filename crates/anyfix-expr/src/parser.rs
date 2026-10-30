use std::iter::Peekable;
use std::vec::IntoIter;

use crate::error::{Error, ErrorCode};
use crate::lexer::lex;
use crate::span::{Span, line_column};
use crate::syntax::{
    BinaryExpression, Element, Expression, Group, GroupKind, Item, Run, Sequence, SyntaxTree,
    Token, TokenKind,
};

/// The deepest bracket nesting accepted; a deeper bracket is [`ErrorCode::TooDeep`].
pub const MAX_DEPTH: usize = 256;

/// The lossless tree and every error found. The tree is complete even when
/// there are errors, so `syntax.to_source()` always returns the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub syntax: SyntaxTree,
    pub errors: Vec<Error>,
}

pub fn parse(source: &str) -> Parsed {
    let (tokens, errors) = lex(source);
    let mut parser = Parser {
        source,
        tokens: tokens.into_iter().peekable(),
        open: Vec::new(),
        errors,
    };
    let sequence = parser.sequence();
    let end = parser.next();
    let mut errors = parser.errors;
    errors.sort_by_key(|error| error.span.start);
    Parsed {
        syntax: SyntaxTree { sequence, end },
        errors,
    }
}

fn opener(kind: &TokenKind) -> Option<GroupKind> {
    match kind {
        TokenKind::OpenParen => Some(GroupKind::Parens),
        TokenKind::OpenBracket => Some(GroupKind::Brackets),
        TokenKind::OpenBrace => Some(GroupKind::Braces),
        _ => None,
    }
}

fn closer(kind: &TokenKind) -> Option<GroupKind> {
    match kind {
        TokenKind::CloseParen => Some(GroupKind::Parens),
        TokenKind::CloseBracket => Some(GroupKind::Brackets),
        TokenKind::CloseBrace => Some(GroupKind::Braces),
        _ => None,
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Peekable<IntoIter<Token>>,
    /// The brackets enclosing the current position, outermost first.
    open: Vec<GroupKind>,
    errors: Vec<Error>,
}

impl Parser<'_> {
    fn peek(&mut self) -> &Token {
        self.tokens.peek().expect("the end token is never consumed")
    }

    fn next(&mut self) -> Token {
        self.tokens.next().expect("the end token is never consumed")
    }

    fn error(&mut self, code: ErrorCode, span: Span, message: String) {
        self.errors.push(Error::new(code, span, message));
    }

    fn at(&self, span: Span) -> String {
        line_column(self.source, span.start).to_string()
    }

    fn at_sequence_end(&mut self) -> bool {
        let inside_group = !self.open.is_empty();
        let kind = &self.peek().kind;
        *kind == TokenKind::End || (inside_group && closer(kind).is_some())
    }

    fn sequence(&mut self) -> Sequence {
        let mut elements = Vec::new();
        while !self.at_sequence_end() {
            if self.peek().kind == TokenKind::Comma {
                let comma = self.next();
                self.error(
                    ErrorCode::EmptyElement,
                    comma.span,
                    "found `,` with no element before it; expected an element between commas (only a trailing comma may follow nothing); remove the extra `,`".into(),
                );
                elements.push(Element {
                    expression: Expression::Run(Run {
                        items: Vec::new(),
                        position: comma.span.start,
                    }),
                    comma: Some(comma),
                });
                continue;
            }
            let expression = self.expression();
            let comma = (self.peek().kind == TokenKind::Comma).then(|| self.next());
            let last = comma.is_none();
            elements.push(Element { expression, comma });
            if last {
                break;
            }
        }
        Sequence { elements }
    }

    /// Runs joined by `.` and `:`: `.` folds left within each `:`-separated
    /// part, then `:` folds right over the parts.
    fn expression(&mut self) -> Expression {
        let mut runs = vec![self.run()];
        let mut operators = Vec::new();
        while matches!(self.peek().kind, TokenKind::Dot | TokenKind::Colon) {
            operators.push(self.next());
            runs.push(self.run());
        }
        if !operators.is_empty() {
            self.report_empty_operands(&mut runs, &operators);
            self.report_extra_underscores(&runs, &operators);
        }

        let mut runs = runs.into_iter();
        let mut current = Expression::Run(runs.next().expect("at least one run"));
        let mut colons: Vec<(Expression, Token)> = Vec::new();
        for (operator, run) in operators.into_iter().zip(runs) {
            let right = Expression::Run(run);
            if operator.kind == TokenKind::Dot {
                current = Expression::Dot(Box::new(BinaryExpression {
                    left: current,
                    operator,
                    right,
                }));
            } else {
                colons.push((current, operator));
                current = right;
            }
        }
        colons
            .into_iter()
            .rev()
            .fold(current, |right, (left, operator)| {
                Expression::Colon(Box::new(BinaryExpression {
                    left,
                    operator,
                    right,
                }))
            })
    }

    /// `runs[i]` sits between `operators[i - 1]` and `operators[i]`.
    fn report_empty_operands(&mut self, runs: &mut [Run], operators: &[Token]) {
        for (index, run) in runs.iter_mut().enumerate() {
            if !run.is_empty() {
                continue;
            }
            let (operator, side, position) = match index {
                0 => (&operators[0], "before", operators[0].span.start),
                _ => (
                    &operators[index - 1],
                    "after",
                    operators[index - 1].span.end,
                ),
            };
            if run.items.is_empty() {
                run.position = position;
            }
            let text = &operator.text;
            self.errors.push(Error::new(
                ErrorCode::EmptyOperand,
                operator.span,
                format!(
                    "found `{text}` with nothing {side} it; expected a run on both sides of `{text}`; add the missing operand or remove `{text}`"
                ),
            ));
        }
    }

    /// A run that receives a value (right of `.`, or left of `:` when no `.`
    /// precedes it) takes it at one `_`; a second `_` has nowhere to go.
    fn report_extra_underscores(&mut self, runs: &[Run], operators: &[Token]) {
        for (index, run) in runs.iter().enumerate() {
            let receives = (index > 0 && operators[index - 1].kind == TokenKind::Dot)
                || (index < operators.len()
                    && operators[index].kind == TokenKind::Colon
                    && (index == 0 || operators[index - 1].kind == TokenKind::Colon));
            if !receives {
                continue;
            }
            let extra = run.items.iter().filter_map(|item| match item {
                Item::Atom(token) if token.kind == TokenKind::Word && token.text == "_" => {
                    Some(token.span)
                }
                _ => None,
            });
            for span in extra.skip(1) {
                self.errors.push(Error::new(
                    ErrorCode::ExtraUnderscore,
                    span,
                    "found a second `_` in a run that receives a value; expected at most one `_` per run; remove it, or put it in brackets to give it its own scope".to_string(),
                ));
            }
        }
    }

    fn run(&mut self) -> Run {
        let position = self.peek().span.start;
        let mut items = Vec::new();
        loop {
            let kind = self.peek().kind.clone();
            match kind {
                TokenKind::Word | TokenKind::Number | TokenKind::String { .. } => {
                    items.push(Item::Atom(self.next()))
                }
                _ if opener(&kind).is_some() => {
                    if self.open.len() >= MAX_DEPTH {
                        self.skip_too_deep(&mut items);
                    } else {
                        items.push(Item::Group(self.group()));
                    }
                }
                _ if closer(&kind).is_some() && self.open.is_empty() => {
                    let token = self.next();
                    let text = &token.text;
                    let message = format!(
                        "found `{text}` with no open bracket to close; expected an opening bracket before it; remove `{text}` or add its opener"
                    );
                    self.error(ErrorCode::UnexpectedCloser, token.span, message);
                    items.push(Item::Skipped(token));
                }
                _ => return Run { items, position },
            }
        }
    }

    /// Keeps a too-deep bracket and everything up to its balancing closer as
    /// skipped tokens, so the nesting never grows the stack.
    fn skip_too_deep(&mut self, items: &mut Vec<Item>) {
        let open = self.next();
        let message = format!(
            "found `{}` nested more than {MAX_DEPTH} brackets deep; expected at most {MAX_DEPTH} levels; flatten the nesting",
            open.text
        );
        self.error(ErrorCode::TooDeep, open.span, message);
        items.push(Item::Skipped(open));
        let mut depth = 1;
        while depth > 0 && self.peek().kind != TokenKind::End {
            let token = self.next();
            if opener(&token.kind).is_some() {
                depth += 1;
            } else if closer(&token.kind).is_some() {
                depth -= 1;
            }
            items.push(Item::Skipped(token));
        }
    }

    fn group(&mut self) -> Group {
        let open = self.next();
        let kind = opener(&open.kind).expect("called on an opener");
        self.open.push(kind);
        let sequence = self.sequence();
        self.open.pop();
        let (open_char, close_char) = (kind.open(), kind.close());
        let opened_at = self.at(open.span);
        let close = match closer(&self.peek().kind) {
            Some(found) if found == kind => Some(self.next()),
            Some(found) if self.open.contains(&found) => {
                let found_char = found.close();
                let message = format!(
                    "found `{open_char}` at {opened_at} with no matching `{close_char}` before the `{found_char}` that closes an outer `{}`; expected `{close_char}`; add `{close_char}` where the group ends",
                    found.open()
                );
                self.error(ErrorCode::UnclosedBracket, open.span, message);
                None
            }
            Some(found) => {
                let token = self.next();
                let found_char = found.close();
                let message = format!(
                    "found `{found_char}` at {} closing `{open_char}` from {opened_at}; expected `{close_char}`; change one of them so the pair matches",
                    self.at(token.span)
                );
                self.error(ErrorCode::MismatchedBracket, token.span, message);
                Some(token)
            }
            None => {
                let message = format!(
                    "found `{open_char}` at {opened_at} with no matching `{close_char}` before the end of input; expected `{close_char}`; add `{close_char}` where the group ends"
                );
                self.error(ErrorCode::UnclosedBracket, open.span, message);
                None
            }
        };
        Group {
            kind,
            open,
            sequence,
            close,
        }
    }
}
