//! Context-free parser. Mixfix calls are NOT resolved here: an expression is a
//! flat sequence of words and single-token atoms (`SeqItem`). The checker
//! matches that sequence against declared signatures.

use crate::ast::*;
use crate::lexer::{Diag, Res, Span, Tok, Token, lex};

pub fn parse_program(src: &str) -> Res<Vec<Item>> {
    let toks = lex(src)?;
    let mut p = Parser { t: toks, p: 0 };
    let mut items = Vec::new();
    loop {
        p.skip_nl();
        match p.peek() {
            Tok::Eof => break,
            Tok::Kw(k) if k == "FUNCTION" || k == "RECORD" || k == "ENUM" => {
                items.push(Item::Decl(p.parse_decl()?));
            }
            _ => {
                items.push(Item::Stmt(p.parse_stmt()?));
                p.end_of_stmt()?;
            }
        }
    }
    Ok(items)
}

struct Parser {
    t: Vec<Token>,
    p: usize,
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Word(w) => format!("`{w}`"),
        Tok::Kw(k) => format!("`{k}`"),
        Tok::Int(n) => n.to_string(),
        Tok::Float(f) => f.to_string(),
        Tok::Str(s) => format!("{s:?}"),
        Tok::LParen => "`(`".into(),
        Tok::RParen => "`)`".into(),
        Tok::LBrace => "`{`".into(),
        Tok::RBrace => "`}`".into(),
        Tok::LBracket => "`[`".into(),
        Tok::RBracket => "`]`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Colon => "`:`".into(),
        Tok::Question => "`?`".into(),
        Tok::Arrow => "`=>`".into(),
        Tok::Pipe => "`|>`".into(),
        Tok::Eq => "`=`".into(),
        Tok::Underscore => "`_`".into(),
        Tok::Newline => "end of line".into(),
        Tok::Eof => "end of input".into(),
    }
}

/// The meta-syntax shared by every call-like construct (signatures, types,
/// patterns, expressions): a run of literal words, `( slot )`s and
/// single-token atoms. A context only decides what a slot and an atom hold.
enum Anyfix<S, A> {
    Word(String, Span),
    Slot(S),
    Atom(A),
}

#[derive(Clone, Copy, Default)]
struct AnyfixRules {
    /// Every word must start uppercase (type names).
    capitalized: bool,
    /// Newlines between parts are insignificant (inside delimiters).
    skip_newlines: bool,
}

fn capitalize(w: &str) -> String {
    let mut cs = w.chars();
    cs.next()
        .map(|c| c.to_uppercase().collect::<String>() + cs.as_str())
        .unwrap_or_default()
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.t[self.p].tok
    }

    fn span(&self) -> Span {
        self.t[self.p].span
    }

    fn bump(&mut self) -> Token {
        let t = self.t[self.p].clone();
        if self.p + 1 < self.t.len() {
            self.p += 1;
        }
        t
    }

    fn skip_nl(&mut self) {
        while matches!(self.peek(), Tok::Newline) {
            self.bump();
        }
    }

    fn err<T>(&self, what: &str) -> Res<T> {
        Err(Diag::new(
            self.span(),
            format!("expected {what}, found {}", describe(self.peek())),
        ))
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Res<Span> {
        if *self.peek() == tok {
            Ok(self.bump().span)
        } else {
            self.err(what)
        }
    }

    fn end_of_stmt(&mut self) -> Res<()> {
        match self.peek() {
            Tok::Newline | Tok::Eof | Tok::RBrace => Ok(()),
            other => Err(Diag::new(
                self.span(),
                format!(
                    "unexpected {} after the end of an expression; put each statement on its own line, and wrap nested calls in parentheses",
                    describe(other)
                ),
            )),
        }
    }

    // ---------- declarations ----------

    fn parse_decl(&mut self) -> Res<Decl> {
        let kw = self.bump();
        let Tok::Kw(kw_name) = kw.tok else {
            unreachable!()
        };
        match kw_name.as_str() {
            "FUNCTION" => {
                let sig = self.parse_sig()?;
                let ret = if matches!(self.peek(), Tok::Colon) {
                    self.bump();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(Tok::LBrace, "`{` to start the function body")?;
                self.skip_nl();
                let body = if matches!(self.peek(), Tok::Kw(k) if k == "NATIVE") {
                    self.bump();
                    let Tok::Str(name) = self.peek().clone() else {
                        return self.err("a native function name string");
                    };
                    self.bump();
                    self.skip_nl();
                    self.expect(Tok::RBrace, "`}`")?;
                    FnBody::Native(name)
                } else {
                    FnBody::Block(self.parse_block()?)
                };
                Ok(Decl::Function { sig, ret, body })
            }
            "RECORD" => {
                let (name, span) = self.parse_decl_name()?;
                self.expect(Tok::LBrace, "`{` after the record name")?;
                let mut fields = Vec::new();
                loop {
                    self.skip_nl();
                    if matches!(self.peek(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    let sig = self.parse_sig()?;
                    if sig.parts.is_empty() {
                        return self.err("a field name");
                    }
                    let optional = if matches!(self.peek(), Tok::Question) {
                        self.bump();
                        true
                    } else {
                        false
                    };
                    self.expect(Tok::Colon, "`:` and a field type")?;
                    let ty = self.parse_type()?;
                    fields.push(FieldAst { sig, optional, ty });
                    self.field_sep()?;
                }
                Ok(Decl::Record { name, fields, span })
            }
            "ENUM" => {
                let (name, span) = self.parse_decl_name()?;
                self.expect(Tok::LBrace, "`{` after the enum name")?;
                let mut variants = Vec::new();
                loop {
                    self.skip_nl();
                    if matches!(self.peek(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    let sig = self.parse_sig()?;
                    if sig.parts.is_empty() {
                        return self.err("a variant");
                    }
                    variants.push(sig);
                    self.field_sep()?;
                }
                Ok(Decl::Enum {
                    name,
                    variants,
                    span,
                })
            }
            _ => unreachable!(),
        }
    }

    fn field_sep(&mut self) -> Res<()> {
        match self.peek() {
            Tok::Comma | Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::RBrace => Ok(()),
            _ => self.err("`,`, a new line or `}`"),
        }
    }

    /// One part of an anyfix run, or `None` if the next token cannot continue it.
    /// `slot` parses what is inside `( … )` (the closing `)` is handled here);
    /// `atom` recognises a context's single-token parts.
    fn anyfix_item<S, A>(
        &mut self,
        rules: AnyfixRules,
        slot: &mut impl FnMut(&mut Self) -> Res<S>,
        atom: &mut impl FnMut(&mut Self) -> Res<Option<A>>,
    ) -> Res<Option<Anyfix<S, A>>> {
        if rules.skip_newlines {
            self.skip_nl();
        }
        let span = self.span();
        match self.peek().clone() {
            Tok::Word(w) => {
                if rules.capitalized {
                    self.check_capitalized(&w)?;
                }
                self.bump();
                Ok(Some(Anyfix::Word(w, span)))
            }
            Tok::LParen => {
                self.bump();
                let s = slot(self)?;
                self.expect(Tok::RParen, "`)`")?;
                Ok(Some(Anyfix::Slot(s)))
            }
            _ => Ok(atom(self)?.map(Anyfix::Atom)),
        }
    }

    fn parse_anyfix<S, A>(
        &mut self,
        rules: AnyfixRules,
        mut slot: impl FnMut(&mut Self) -> Res<S>,
        mut atom: impl FnMut(&mut Self) -> Res<Option<A>>,
    ) -> Res<Vec<Anyfix<S, A>>> {
        let mut parts = Vec::new();
        while let Some(part) = self.anyfix_item(rules, &mut slot, &mut atom)? {
            parts.push(part);
        }
        Ok(parts)
    }

    fn no_atom(_: &mut Self) -> Res<Option<()>> {
        Ok(None)
    }

    /// Slot-less run of words (declaration names, brace field keys).
    fn parse_words(&mut self, rules: AnyfixRules) -> Res<Vec<String>> {
        let parts = self.parse_anyfix(
            rules,
            |p| p.err::<()>("a name without parameters"),
            Self::no_atom,
        )?;
        Ok(parts
            .into_iter()
            .filter_map(|p| match p {
                Anyfix::Word(w, _) => Some(w),
                _ => None,
            })
            .collect())
    }

    fn parse_decl_name(&mut self) -> Res<(String, Span)> {
        let span = self.span();
        let words = self.parse_words(AnyfixRules {
            capitalized: true,
            ..Default::default()
        })?;
        if words.is_empty() {
            return self.err("a name");
        }
        Ok((words.join(" "), span))
    }

    fn parse_sig(&mut self) -> Res<SigAst> {
        let span = self.span();
        let parts = self
            .parse_anyfix(
                AnyfixRules::default(),
                |p| {
                    let Tok::Word(name) = p.peek().clone() else {
                        return p.err("a parameter name");
                    };
                    p.bump();
                    let ty = if matches!(p.peek(), Tok::Colon) {
                        p.bump();
                        Some(p.parse_type()?)
                    } else {
                        None
                    };
                    Ok(Part::Param { name, ty })
                },
                Self::no_atom,
            )?
            .into_iter()
            .filter_map(|p| match p {
                Anyfix::Word(w, _) => Some(Part::Word(w)),
                Anyfix::Slot(param) => Some(param),
                Anyfix::Atom(()) => None,
            })
            .collect();
        Ok(SigAst { parts, span })
    }

    /// An anyfix type: capitalized words and `(Type)` slots, e.g. `Result (List (Entry)) Or (String)`.
    /// `name` keeps the words with `_` standing in for each slot (`Result _ Or _`).
    fn parse_type(&mut self) -> Res<TyExpr> {
        let span = self.span();
        let parts = self.parse_anyfix(
            AnyfixRules {
                capitalized: true,
                ..Default::default()
            },
            |p| p.parse_type(),
            Self::no_atom,
        )?;
        if !parts.iter().any(|p| matches!(p, Anyfix::Word(..))) {
            return self.err("a type");
        }
        let mut names = Vec::new();
        let mut args = Vec::new();
        for part in parts {
            match part {
                Anyfix::Word(w, _) => names.push(w),
                Anyfix::Slot(t) => {
                    names.push("_".into());
                    args.push(t);
                }
                Anyfix::Atom(()) => {}
            }
        }
        Ok(TyExpr {
            name: names.join(" "),
            args,
            span,
        })
    }

    fn check_capitalized(&self, w: &str) -> Res<()> {
        if w.chars().next().is_some_and(|c| c.is_uppercase()) {
            Ok(())
        } else {
            Err(Diag::new(
                self.span(),
                format!(
                    "type names capitalize every word; write `{}` here",
                    capitalize(w)
                ),
            ))
        }
    }

    // ---------- statements & expressions ----------

    /// Parses statements until the closing `}` (consumed).
    fn parse_block(&mut self) -> Res<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_nl();
            if matches!(self.peek(), Tok::RBrace) {
                self.bump();
                return Ok(stmts);
            }
            if matches!(self.peek(), Tok::Eof) {
                return self.err("`}` to close the block");
            }
            stmts.push(self.parse_stmt()?);
            self.end_of_stmt()?;
        }
    }

    fn parse_stmt(&mut self) -> Res<Stmt> {
        if matches!(self.peek(), Tok::Kw(k) if k == "LET") {
            let span = self.bump().span;
            let Tok::Word(name) = self.peek().clone() else {
                return self.err("a variable name (a single word)");
            };
            self.bump();
            if !matches!(self.peek(), Tok::Eq) {
                return self.err("`=` (variable names are a single word)");
            }
            self.bump();
            let e = self.parse_pipeline(true)?;
            Ok(Stmt::Let(name, e, span))
        } else {
            Ok(Stmt::Expr(self.parse_pipeline(true)?))
        }
    }

    /// `nl`: a newline ends the expression (statement level). Inside
    /// delimiters newlines are insignificant.
    fn parse_pipeline(&mut self, nl: bool) -> Res<Expr> {
        let mut e = self.parse_seq(nl)?;
        loop {
            let save = self.p;
            self.skip_nl();
            if matches!(self.peek(), Tok::Pipe) {
                let span = self.bump().span;
                self.skip_nl();
                let r = self.parse_seq(nl)?;
                e = Expr {
                    k: ExprKind::Pipe(Box::new(e), Box::new(r)),
                    span,
                };
            } else {
                self.p = save;
                break;
            }
        }
        Ok(e)
    }

    fn parse_seq(&mut self, nl: bool) -> Res<Expr> {
        if !nl {
            self.skip_nl();
        }
        let span = self.span();
        match self.peek() {
            Tok::Kw(k) if k == "IF" => return self.parse_if(nl),
            Tok::Kw(k) if k == "MATCH" => return self.parse_match(),
            _ => {}
        }
        let rules = AnyfixRules {
            skip_newlines: !nl,
            ..Default::default()
        };
        let mut items: Vec<SeqItem> = self
            .parse_anyfix(rules, Self::expr_slot, Self::expr_atom)?
            .into_iter()
            .map(Self::seq_item)
            .collect();
        if items.is_empty() {
            return self.err("an expression");
        }
        if items.len() == 1
            && let SeqItem::Atom(_) = &items[0]
        {
            let Some(SeqItem::Atom(e)) = items.pop() else {
                unreachable!()
            };
            return Ok(e);
        }
        Ok(Expr {
            k: ExprKind::Seq(items),
            span,
        })
    }

    fn parse_if(&mut self, nl: bool) -> Res<Expr> {
        let span = self.bump().span;
        let cond = self.parse_pipeline(false)?;
        self.skip_nl();
        if !matches!(self.peek(), Tok::Kw(k) if k == "THEN") {
            return self.err("`THEN`");
        }
        self.bump();
        let then = self.parse_pipeline(false)?;
        self.skip_nl();
        if !matches!(self.peek(), Tok::Kw(k) if k == "ELSE") {
            return self.err("`ELSE`");
        }
        self.bump();
        let els = self.parse_pipeline_branch(nl)?;
        Ok(Expr {
            k: ExprKind::If(Box::new(cond), Box::new(then), Box::new(els)),
            span,
        })
    }

    fn parse_pipeline_branch(&mut self, nl: bool) -> Res<Expr> {
        self.parse_pipeline(nl)
    }

    fn parse_match(&mut self) -> Res<Expr> {
        let span = self.bump().span;
        let scrut = match self.anyfix_item(
            AnyfixRules::default(),
            &mut Self::expr_slot,
            &mut Self::expr_atom,
        )? {
            Some(Anyfix::Word(w, s)) => Expr {
                k: ExprKind::Seq(vec![SeqItem::Word(w, s)]),
                span: s,
            },
            Some(Anyfix::Slot(e) | Anyfix::Atom(e)) => e,
            None => return self.err("a single token or parenthesised expression after MATCH"),
        };
        self.skip_nl();
        self.expect(Tok::LBrace, "`{` after the MATCH subject")?;
        let mut arms = Vec::new();
        loop {
            self.skip_nl();
            while matches!(self.peek(), Tok::Comma) {
                self.bump();
                self.skip_nl();
            }
            if matches!(self.peek(), Tok::RBrace) {
                self.bump();
                break;
            }
            let aspan = self.span();
            let pat: Vec<PatItem> = self
                .parse_anyfix(
                    AnyfixRules::default(),
                    |p| match p.peek().clone() {
                        Tok::Word(w) => {
                            p.bump();
                            Ok(w)
                        }
                        Tok::Underscore => {
                            p.bump();
                            Ok("_".to_string())
                        }
                        _ => p.err("a variable name to bind"),
                    },
                    |p| {
                        Ok(matches!(p.peek(), Tok::Underscore).then(|| {
                            p.bump();
                        }))
                    },
                )?
                .into_iter()
                .map(|part| match part {
                    Anyfix::Word(w, _) => PatItem::Word(w),
                    Anyfix::Atom(()) => PatItem::Wild,
                    Anyfix::Slot(name) => PatItem::Bind(name),
                })
                .collect();
            if pat.is_empty() {
                return self.err("a pattern");
            }
            self.expect(Tok::Arrow, "`=>`")?;
            let body = self.parse_pipeline(true)?;
            arms.push(Arm {
                pat,
                body,
                span: aspan,
            });
        }
        Ok(Expr {
            k: ExprKind::Match(Box::new(scrut), arms),
            span,
        })
    }

    /// `( … )` in an expression: a full parenthesised expression.
    fn expr_slot(&mut self) -> Res<Expr> {
        let e = self.parse_pipeline(false)?;
        self.skip_nl();
        Ok(e)
    }

    fn seq_item(part: Anyfix<Expr, Expr>) -> SeqItem {
        match part {
            Anyfix::Word(w, span) => SeqItem::Word(w, span),
            Anyfix::Slot(e) | Anyfix::Atom(e) => SeqItem::Atom(e),
        }
    }

    /// Single-token expression parts: literals, `_`, list and brace literals.
    fn expr_atom(&mut self) -> Res<Option<Expr>> {
        let span = self.span();
        let mk = |k: ExprKind| Some(Expr { k, span });
        Ok(match self.peek().clone() {
            Tok::Underscore => {
                self.bump();
                mk(ExprKind::Placeholder)
            }
            Tok::Int(n) => {
                self.bump();
                mk(ExprKind::Int(n))
            }
            Tok::Float(f) => {
                self.bump();
                mk(ExprKind::Float(f))
            }
            Tok::Str(s) => {
                self.bump();
                mk(ExprKind::Str(s))
            }
            Tok::LBracket => {
                self.bump();
                let mut xs = Vec::new();
                loop {
                    self.skip_nl();
                    if matches!(self.peek(), Tok::RBracket) {
                        self.bump();
                        break;
                    }
                    xs.push(self.parse_pipeline(false)?);
                    self.skip_nl();
                    if matches!(self.peek(), Tok::Comma) {
                        self.bump();
                    } else if !matches!(self.peek(), Tok::RBracket) {
                        return self.err("`,` or `]`");
                    }
                }
                mk(ExprKind::List(xs))
            }
            Tok::LBrace => {
                self.bump();
                let mut fields = Vec::new();
                loop {
                    self.skip_nl();
                    if matches!(self.peek(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    let fspan = self.span();
                    let key = self.parse_words(AnyfixRules::default())?;
                    if key.is_empty() {
                        return self.err("a field name");
                    }
                    let value = if matches!(self.peek(), Tok::Colon) {
                        self.bump();
                        Some(self.parse_pipeline(false)?)
                    } else {
                        None
                    };
                    fields.push(BraceField {
                        key,
                        value,
                        span: fspan,
                    });
                    self.skip_nl();
                    if matches!(self.peek(), Tok::Comma) {
                        self.bump();
                    } else if !matches!(self.peek(), Tok::RBrace) {
                        return self.err("`,` or `}`");
                    }
                }
                mk(ExprKind::Brace(fields))
            }
            _ => None,
        })
    }
}
