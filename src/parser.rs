//! Context-free parser. Mixfix calls are NOT resolved here: an expression is a
//! flat sequence of words and single-token atoms (`SeqItem`). The checker
//! matches that sequence against declared signatures.

use std::collections::HashMap;

use crate::ast::*;
use crate::lexer::{Diag, Res, Span, Tok, Token, lex_with_docs};

pub fn parse_program(src: &str) -> Res<Vec<Item>> {
    let (toks, docs) = lex_with_docs(src)?;
    let mut p = Parser {
        t: toks,
        p: 0,
        docs,
    };
    let mut items: Vec<Item> = Vec::new();
    loop {
        while matches!(p.peek(), Tok::Semi) {
            p.bump();
        }
        match p.peek() {
            Tok::Eof => break,
            Tok::Kw(k) if k == "NAMESPACE" => {
                let span = p.span();
                let decl = p.parse_decl()?;
                if items
                    .iter()
                    .any(|i| matches!(i, Item::Decl(Decl::Namespace { .. })))
                {
                    return Err(Diag::new(
                        span,
                        "a file has at most one NAMESPACE; remove this one or move its declarations to another file",
                    ));
                }
                if !items.is_empty() {
                    return Err(Diag::new(
                        span,
                        "NAMESPACE must be the first declaration in the file; move it above everything else",
                    ));
                }
                items.push(Item::Decl(decl));
            }
            Tok::Kw(k) if k == "FUNCTION" || k == "RECORD" || k == "ENUM" => {
                items.push(Item::Decl(p.parse_decl()?));
            }
            _ => {
                let stmt = p.parse_stmt()?;
                p.end_of_stmt(&stmt)?;
                items.push(Item::Stmt(stmt));
            }
        }
    }
    Ok(items)
}

struct Parser {
    t: Vec<Token>,
    p: usize,
    /// Doc comments by the line of the token they sit above.
    docs: HashMap<usize, String>,
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
        Tok::ColonColon => "`::`".into(),
        Tok::Ellipsis => "`...`".into(),
        Tok::Question => "`?`".into(),
        Tok::Arrow => "`=>`".into(),
        Tok::Pipe => "`|>`".into(),
        Tok::Eq => "`=`".into(),
        Tok::Underscore => "`_`".into(),
        Tok::Semi => "`;`".into(),
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

    /// Every statement ends with `;`. A scope block `ns:: { ... }` is closed by
    /// its `}` and may omit it. Returns whether a `;` was consumed.
    fn end_of_stmt(&mut self, stmt: &Stmt) -> Res<bool> {
        match self.peek() {
            Tok::Semi => {
                self.bump();
                Ok(true)
            }
            _ if matches!(stmt, Stmt::Scope(..)) => Ok(false),
            other => Err(Diag::new(
                self.span(),
                format!(
                    "expected `;` to end the statement, found {}; every statement ends with `;`, and nested calls need parentheses",
                    describe(other)
                ),
            )),
        }
    }

    fn parse_decl(&mut self) -> Res<Decl> {
        let kw = self.bump();
        let doc = self.docs.get(&kw.span.line).cloned();
        let Tok::Kw(kw_name) = kw.tok else {
            unreachable!()
        };
        match kw_name.as_str() {
            "NAMESPACE" => {
                let span = self.span();
                let words = self.parse_words(AnyfixRules::default())?;
                match words.as_slice() {
                    [name] => Ok(Decl::Namespace {
                        name: name.clone(),
                        span,
                    }),
                    [] => self.err("a namespace name"),
                    _ => Err(Diag::new(
                        span,
                        "a namespace name is a single word, e.g. `NAMESPACE fd`",
                    )),
                }
            }
            "FUNCTION" => {
                let sig = self.parse_sig(false)?;
                let ret = if matches!(self.peek(), Tok::Colon) {
                    self.bump();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(Tok::LBrace, "`{` to start the function body")?;
                let body = if matches!(self.peek(), Tok::Kw(k) if k == "NATIVE") {
                    self.bump();
                    let Tok::Str(name) = self.peek().clone() else {
                        return self.err("a native function name string");
                    };
                    self.bump();
                    self.expect(Tok::RBrace, "`}`")?;
                    FnBody::Native(name)
                } else {
                    FnBody::Block(self.parse_block()?)
                };
                Ok(Decl::Function {
                    sig,
                    ret,
                    body,
                    doc,
                })
            }
            "RECORD" => {
                let (name, span) = self.parse_decl_name()?;
                self.expect(Tok::LBrace, "`{` after the record name")?;
                let mut fields = Vec::new();
                loop {
                    if matches!(self.peek(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    fields.push(self.parse_field()?);
                    self.field_sep()?;
                }
                Ok(Decl::Record {
                    name,
                    fields,
                    span,
                    doc,
                })
            }
            "ENUM" => {
                let (name, span) = self.parse_decl_name()?;
                self.expect(Tok::LBrace, "`{` after the enum name")?;
                let mut variants = Vec::new();
                loop {
                    if matches!(self.peek(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    let sig = self.parse_sig(true)?;
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
                    doc,
                })
            }
            _ => unreachable!(),
        }
    }

    /// A record field: words and type-only slots, each slot optionally
    /// `DEFAULT value`, e.g. `port (Int DEFAULT 8080)`.
    fn parse_field(&mut self) -> Res<FieldAst> {
        let span = self.span();
        let parts: Vec<FieldPart> = self
            .parse_anyfix(
                AnyfixRules::default(),
                |p| {
                    let ty = p.parse_type()?;
                    let default = if matches!(p.peek(), Tok::Kw(k) if k == "DEFAULT") {
                        p.bump();
                        Some(p.expr_slot()?)
                    } else {
                        None
                    };
                    Ok((ty, default))
                },
                Self::no_atom,
            )?
            .into_iter()
            .filter_map(|part| match part {
                Anyfix::Word(w, _) => Some(FieldPart::Word(w)),
                Anyfix::Slot((ty, default)) => Some(FieldPart::Slot { ty, default }),
                Anyfix::Atom(()) => None,
            })
            .collect();
        if !parts.iter().any(|p| matches!(p, FieldPart::Word(_))) {
            return self.err("a field name (a field needs at least one word)");
        }
        Ok(FieldAst { parts, span })
    }

    fn field_sep(&mut self) -> Res<()> {
        match self.peek() {
            Tok::Comma => {
                self.bump();
                Ok(())
            }
            Tok::RBrace => Ok(()),
            _ => self.err("`,` or `}`"),
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
        let span = self.span();
        match self.peek().clone() {
            Tok::Word(w) => {
                if matches!(self.t[self.p + 1].tok, Tok::ColonColon) {
                    return Err(Diag::new(
                        span,
                        format!(
                            "`{w}::` qualifies a whole call, so it must come first in the call; write `{w}::` before the call's first word or slot"
                        ),
                    ));
                }
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

    /// Leading `ns::` qualifiers of a call or type, outermost first.
    fn parse_qualifiers(&mut self) -> Vec<(String, Span)> {
        let mut quals = Vec::new();
        while let Tok::Word(w) = self.peek().clone()
            && matches!(self.t[self.p + 1].tok, Tok::ColonColon)
        {
            let span = self.bump().span;
            self.bump();
            quals.push((w, span));
        }
        quals
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

    /// `bare_types` (enum variants): a slot may be a type alone, `max depth (Int)`;
    /// it gets the placeholder name `_`.
    fn parse_sig(&mut self, bare_types: bool) -> Res<SigAst> {
        let span = self.span();
        let parts = self
            .parse_anyfix(
                AnyfixRules::default(),
                |p| {
                    if matches!(p.peek(), Tok::LParen) {
                        return p.parse_tuple_param();
                    }
                    let Tok::Word(name) = p.peek().clone() else {
                        return p.err("a parameter name");
                    };
                    if bare_types && !matches!(p.t[(p.p + 1).min(p.t.len() - 1)].tok, Tok::Colon) {
                        let ty = p.parse_type()?;
                        return Ok(Part::Param {
                            name: "_".into(),
                            ty: Some(ty),
                        });
                    }
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

    /// `(name: Type, name: Type)`: a tuple parameter's components, two or more.
    fn parse_tuple_param(&mut self) -> Res<Part> {
        let span = self.expect(Tok::LParen, "`(`")?;
        let mut comps = Vec::new();
        loop {
            let Tok::Word(name) = self.peek().clone() else {
                return self.err("a component name");
            };
            self.bump();
            if !matches!(self.peek(), Tok::Colon) {
                return self.err("`:` and the component's type");
            }
            self.bump();
            comps.push((name, self.parse_type()?));
            if matches!(self.peek(), Tok::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(Tok::RParen, "`,` or `)`")?;
        if comps.len() < 2 {
            return Err(Diag::new(
                span,
                "a tuple parameter has two or more components, e.g. `((command: String, args: List (String)))`",
            ));
        }
        Ok(Part::Tuple(comps))
    }

    /// An anyfix type: capitalized words and `(Type)` slots, e.g. `Result (List (Entry)) Or (String)`.
    /// `name` keeps the words with `_` standing in for each slot (`Result _ Or _`).
    fn parse_type(&mut self) -> Res<TyExpr> {
        let span = self.span();
        let qualifiers: Vec<String> = self
            .parse_qualifiers()
            .into_iter()
            .map(|(q, _)| q)
            .collect();
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
            qualifiers,
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
    /// A block's last expression is its value unless it ends with `;`, which
    /// discards it.
    fn parse_block(&mut self) -> Res<Vec<Stmt>> {
        let mut stmts = Vec::new();
        let mut terminated = false;
        loop {
            while matches!(self.peek(), Tok::Semi) {
                self.bump();
            }
            if matches!(self.peek(), Tok::RBrace) {
                self.bump();
                if terminated && let Some(Stmt::Expr(e)) = stmts.pop() {
                    stmts.push(Stmt::Discard(e));
                }
                return Ok(stmts);
            }
            if matches!(self.peek(), Tok::Eof) {
                return self.err("`}` to close the block");
            }
            let stmt = self.parse_stmt()?;
            terminated = if matches!(self.peek(), Tok::RBrace) && !matches!(stmt, Stmt::Let(..)) {
                false
            } else {
                self.end_of_stmt(&stmt)?
            };
            stmts.push(stmt);
        }
    }

    fn parse_stmt(&mut self) -> Res<Stmt> {
        if let Tok::Word(ns) = self.peek().clone()
            && matches!(
                self.t.get(self.p + 1).map(|t| &t.tok),
                Some(Tok::ColonColon)
            )
            && matches!(self.t.get(self.p + 2).map(|t| &t.tok), Some(Tok::LBrace))
        {
            let span = self.bump().span;
            self.bump();
            self.bump();
            return Ok(Stmt::Scope(ns, self.parse_block()?, span));
        }
        if matches!(self.peek(), Tok::Kw(k) if k == "LET") {
            let span = self.bump().span;
            let Tok::Word(name) = self.peek().clone() else {
                return self.err("a variable name (a single word)");
            };
            self.bump();
            // `LET name: Type = e` means `LET name = (e : Type)`.
            let ty = if matches!(self.peek(), Tok::Colon) {
                self.bump();
                Some(self.parse_type()?)
            } else {
                None
            };
            if !matches!(self.peek(), Tok::Eq) {
                return self.err("`=` (variable names are a single word)");
            }
            self.bump();
            let mut e = self.parse_pipeline()?;
            if let Some(ty) = ty {
                e = Expr {
                    k: ExprKind::Ascribe(Box::new(e), ty),
                    span,
                };
            }
            Ok(Stmt::Let(name, e, span))
        } else {
            Ok(Stmt::Expr(self.parse_pipeline()?))
        }
    }

    fn parse_pipeline(&mut self) -> Res<Expr> {
        let mut e = self.parse_seq()?;
        e = self.parse_ascription(e)?;
        loop {
            let save = self.p;
            if matches!(self.peek(), Tok::Pipe) {
                let span = self.bump().span;
                let r = self.parse_seq()?;
                let r = self.parse_ascription(r)?;
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

    /// Optional `: Type` after an expression.
    fn parse_ascription(&mut self, e: Expr) -> Res<Expr> {
        if !matches!(self.peek(), Tok::Colon) {
            return Ok(e);
        }
        let span = self.bump().span;
        let ty = self.parse_type()?;
        Ok(Expr {
            k: ExprKind::Ascribe(Box::new(e), ty),
            span,
        })
    }

    fn parse_seq(&mut self) -> Res<Expr> {
        let span = self.span();
        match self.peek() {
            Tok::Kw(k) if k == "IF" => return self.parse_if(),
            Tok::Kw(k) if k == "MATCH" => return self.parse_match(),
            _ => {}
        }
        let quals = self.parse_qualifiers();
        let mut items: Vec<SeqItem> = self
            .parse_anyfix(AnyfixRules::default(), Self::expr_slot, Self::expr_atom)?
            .into_iter()
            .map(Self::seq_item)
            .collect();
        if items.is_empty() {
            return self.err(if quals.is_empty() {
                "an expression"
            } else {
                "a call after the `::` qualifier"
            });
        }
        let mut e = if items.len() == 1
            && let SeqItem::Atom(_) = &items[0]
        {
            let Some(SeqItem::Atom(e)) = items.pop() else {
                unreachable!()
            };
            e
        } else {
            Expr {
                k: ExprKind::Seq(items),
                span,
            }
        };
        for (q, qspan) in quals.into_iter().rev() {
            e = Expr {
                k: ExprKind::Qualified(q, Box::new(e)),
                span: qspan,
            };
        }
        Ok(e)
    }

    fn parse_if(&mut self) -> Res<Expr> {
        let span = self.bump().span;
        let cond = self.parse_pipeline()?;
        if !matches!(self.peek(), Tok::Kw(k) if k == "THEN") {
            return self.err("`THEN`");
        }
        self.bump();
        let then = self.parse_pipeline()?;
        if !matches!(self.peek(), Tok::Kw(k) if k == "ELSE") {
            return self.err("`ELSE`");
        }
        self.bump();
        let els = self.parse_pipeline()?;
        Ok(Expr {
            k: ExprKind::If(Box::new(cond), Box::new(then), Box::new(els)),
            span,
        })
    }

    fn parse_match(&mut self) -> Res<Expr> {
        let span = self.bump().span;
        let scrut =
            self.single_operand("a single token or parenthesised expression after MATCH")?;
        self.expect(Tok::LBrace, "`{` after the MATCH subject")?;
        let mut arms = Vec::new();
        loop {
            while matches!(self.peek(), Tok::Comma) {
                self.bump();
            }
            if matches!(self.peek(), Tok::RBrace) {
                self.bump();
                break;
            }
            let aspan = self.span();
            let pat = self.parse_pattern()?;
            if pat.is_empty() {
                return self.err("a pattern");
            }
            self.expect(Tok::Arrow, "`=>`")?;
            let body = self.parse_pipeline()?;
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

    /// A MATCH pattern: words, `(name)` binders, `_`, or a `{ … }` record pattern.
    fn parse_pattern(&mut self) -> Res<Vec<PatItem>> {
        Ok(self
            .parse_anyfix(
                AnyfixRules::default(),
                |p| {
                    let mut inner = p.parse_pattern()?;
                    match inner.as_mut_slice() {
                        [] => p.err("a variable name to bind"),
                        [PatItem::Word(w)] => Ok(PatItem::Bind(std::mem::take(w))),
                        [PatItem::Wild] => Ok(PatItem::Bind("_".into())),
                        _ => Ok(PatItem::Nested(inner)),
                    }
                },
                |p| match p.peek() {
                    Tok::Underscore => {
                        p.bump();
                        Ok(Some(PatItem::Wild))
                    }
                    Tok::LBrace => Ok(Some(PatItem::Record(p.parse_record_pattern()?))),
                    _ => Ok(None),
                },
            )?
            .into_iter()
            .map(|part| match part {
                Anyfix::Word(w, _) => PatItem::Word(w),
                Anyfix::Atom(item) => item,
                Anyfix::Slot(item) => item,
            })
            .collect())
    }

    /// `{ field pattern, field pattern }`, one anyfix pattern per field.
    fn parse_record_pattern(&mut self) -> Res<Vec<Vec<PatItem>>> {
        self.bump();
        let mut entries = Vec::new();
        loop {
            if matches!(self.peek(), Tok::RBrace) {
                self.bump();
                return Ok(entries);
            }
            let entry = self.parse_pattern()?;
            if entry.is_empty() {
                return self.err("a field pattern");
            }
            entries.push(entry);
            if matches!(self.peek(), Tok::Comma) {
                self.bump();
            } else if !matches!(self.peek(), Tok::RBrace) {
                return self.err("`,` or `}`");
            }
        }
    }

    /// Exactly one anyfix part used as a value: a lone word (a variable or a
    /// zero-parameter call), an atom or a parenthesised expression.
    fn single_operand(&mut self, what: &str) -> Res<Expr> {
        match self.anyfix_item(
            AnyfixRules::default(),
            &mut Self::expr_slot,
            &mut Self::expr_atom,
        )? {
            Some(Anyfix::Word(w, s)) => Ok(Expr {
                k: ExprKind::Seq(vec![SeqItem::Word(w, s)]),
                span: s,
            }),
            Some(Anyfix::Slot(e) | Anyfix::Atom(e)) => Ok(e),
            None => self.err(what),
        }
    }

    /// `( … )` in an expression: a full parenthesised expression, or, when
    /// commas separate two or more of them, a tuple.
    fn expr_slot(&mut self) -> Res<Expr> {
        let span = self.span();
        let first = self.slot_element()?;
        if !matches!(self.peek(), Tok::Comma) {
            return Ok(first);
        }
        let mut elems = vec![first];
        while matches!(self.peek(), Tok::Comma) {
            self.bump();
            elems.push(self.slot_element()?);
        }
        Ok(Expr {
            k: ExprKind::Tuple(elems),
            span,
        })
    }

    fn slot_element(&mut self) -> Res<Expr> {
        let e = self.parse_pipeline()?;
        if matches!(self.peek(), Tok::Arrow) {
            return self.lambda(e);
        }
        Ok(e)
    }

    /// `(x) => body`, with the parameter list `(x)` already parsed as the
    /// call `params`. The body runs to the end of the enclosing slot.
    fn lambda(&mut self, params: Expr) -> Res<Expr> {
        let span = params.span;
        // A lone parenthesized word collapses to the call `[Word]`.
        let name = match &params.k {
            ExprKind::Seq(items) => match items.as_slice() {
                [SeqItem::Word(w, _)] => Some(w.clone()),
                _ => None,
            },
            _ => None,
        };
        let Some(name) = name else {
            return Err(Diag::new(
                span,
                "a lambda is written `(name) => body`: exactly one parameter name in parentheses",
            ));
        };
        self.expect(Tok::Arrow, "`=>`")?;
        let body = self.parse_pipeline()?;
        Ok(Expr {
            k: ExprKind::Lambda(name, Box::new(body)),
            span,
        })
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
                    if matches!(self.peek(), Tok::RBracket) {
                        self.bump();
                        break;
                    }
                    if matches!(self.peek(), Tok::Ellipsis) {
                        let sspan = self.bump().span;
                        let operand = self
                            .single_operand("a single token or parenthesised list after `...`")?;
                        xs.push(Expr {
                            k: ExprKind::Spread(Box::new(operand)),
                            span: sspan,
                        });
                    } else {
                        xs.push(self.parse_pipeline()?);
                    }
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
                    if matches!(self.peek(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    let espan = self.span();
                    let items: Vec<SeqItem> = self
                        .parse_anyfix(AnyfixRules::default(), Self::expr_slot, Self::expr_atom)?
                        .into_iter()
                        .map(Self::seq_item)
                        .collect();
                    if items.is_empty() {
                        return self.err("a field entry");
                    }
                    fields.push(BraceEntry { items, span: espan });
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
