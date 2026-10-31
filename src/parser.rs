//! Reads resh source as anyfix expressions (`crates/anyfix-expr`) and turns
//! the tree into resh's AST. The notation does all the grouping; this module
//! only gives the calls a meaning: uppercase keywords (`FUNCTION`, `MATCH`,
//! ...) mark the language's own forms, and every other call is flattened into
//! a `Seq` of words and arguments. Calls are NOT resolved here: the checker
//! matches each `Seq` against the declared signatures.

use std::collections::HashMap;

use anyfix_expr::{NamePart, Node, NodeKind};

use crate::ast::*;
use crate::diag::{Diag, Res, Span};

/// The words resh gives a meaning to. Keywords are uppercase, so a lowercase
/// word (`match`, `if`) stays free for user names.
pub const KEYWORDS: &[&str] = &[
    "FUNCTION",
    "RECORD",
    "ENUM",
    "TYPE",
    "NAMESPACE",
    "IMPORT",
    "MATCH",
    "LET",
    "IF",
    "THEN",
    "ELSE",
    "NATIVE",
    "DEFAULT",
    "AS",
];

pub fn parse_program(src: &str) -> Res<Vec<Item>> {
    let parsed = anyfix_expr::parse(src);
    let reader = Reader {
        src,
        docs: doc_comments(src),
    };
    if let Some(error) = parsed.errors.first() {
        let at = error.span.start;
        let double_colon = src.get(at..).is_some_and(|r| r.starts_with("::"))
            || (at > 0 && src.get(at - 1..).is_some_and(|r| r.starts_with("::")));
        let msg = if double_colon {
            "a namespace qualifier is written with `.`: `git.status`, and a scope block `git.{ ... }`".to_string()
        } else {
            error.message.clone()
        };
        return Err(Diag::new(reader.at(error.span), msg));
    }
    let mut items = Vec::new();
    for node in parsed.syntax.nodes() {
        reader.top_level(&node, &mut items)?;
    }
    Ok(items)
}

/// Doc comments: a run of `##` lines, each alone on its line, directly above a
/// line of code (no blank line between), keyed by that line's number.
fn doc_comments(src: &str) -> HashMap<usize, String> {
    let mut docs = HashMap::new();
    let mut run: Vec<String> = Vec::new();
    for (index, line) in src.lines().enumerate() {
        let text = line.trim_start();
        if let Some(doc) = text.strip_prefix("##") {
            run.push(doc.strip_prefix(' ').unwrap_or(doc).trim_end().to_string());
        } else if text.is_empty() || text.starts_with('#') {
            run.clear();
        } else if !run.is_empty() {
            docs.insert(index + 1, std::mem::take(&mut run).join("\n"));
        }
    }
    docs
}

/// One item of a call read left to right: a name word, or the argument that
/// fills the next slot. `list file in (dir)` is `list`, `file`, `in`, `dir`.
#[derive(Clone, Copy)]
enum Flat<'a> {
    Word(&'a str, anyfix_expr::Span),
    Arg(&'a Node),
}

fn flatten(node: &Node) -> Vec<Flat<'_>> {
    match &node.kind {
        NodeKind::Call(call) => {
            let mut arguments = call.arguments.iter();
            call.name
                .iter()
                .map(|part| match part {
                    NamePart::Word { text, span } => Flat::Word(text, *span),
                    NamePart::Slot => Flat::Arg(arguments.next().expect("one argument per slot")),
                })
                .collect()
        }
        _ => vec![Flat::Arg(node)],
    }
}

fn flat_span(item: &Flat) -> anyfix_expr::Span {
    match item {
        Flat::Word(_, span) => *span,
        Flat::Arg(node) => node.span,
    }
}

/// The first word of a call, if it starts with one: its keyword, if any.
fn head<'a>(items: &[Flat<'a>]) -> Option<&'a str> {
    match items.first() {
        Some(Flat::Word(w, _)) => Some(w),
        _ => None,
    }
}

/// A call that is one word and nothing else: a variable, a namespace or a name.
fn single_word(node: &Node) -> Option<&str> {
    match flatten(node).as_slice() {
        [Flat::Word(w, _)] => Some(w),
        _ => None,
    }
}

fn is_symbol(word: &str) -> bool {
    !word.starts_with(|c: char| c.is_alphanumeric() || c == '_')
}

fn is_lowercase_word(word: &str) -> bool {
    word.starts_with(|c: char| c.is_lowercase())
}

fn braces(node: &Node) -> Option<&[Node]> {
    match &node.kind {
        NodeKind::Braces(nodes) => Some(nodes),
        _ => None,
    }
}

fn capitalize(w: &str) -> String {
    let mut cs = w.chars();
    cs.next()
        .map(|c| c.to_uppercase().collect::<String>() + cs.as_str())
        .unwrap_or_default()
}

const SEPARATE: &str = "declarations and statements are separated by `,`";

struct Reader<'s> {
    src: &'s str,
    /// Doc comments by the line of the code they sit above.
    docs: HashMap<usize, String>,
}

impl Reader<'_> {
    fn at(&self, span: anyfix_expr::Span) -> Span {
        let at = anyfix_expr::line_column(self.src, span.start);
        Span {
            line: at.line,
            col: at.column,
        }
    }

    fn err<T>(&self, span: anyfix_expr::Span, msg: impl Into<String>) -> Res<T> {
        Err(Diag::new(self.at(span), msg))
    }

    fn top_level(&self, node: &Node, items: &mut Vec<Item>) -> Res<()> {
        let flat = flatten(node);
        let span = self.at(node.span);
        match head(&flat) {
            Some("NAMESPACE") => {
                let name = self.single_name(
                    &flat,
                    "a namespace name is a single word, e.g. `NAMESPACE fd`",
                )?;
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
                items.push(Item::Decl(Decl::Namespace { name, span }));
            }
            Some("IMPORT") => {
                let name =
                    self.single_name(&flat, "a library name is a single word, e.g. `IMPORT git`")?;
                items.push(Item::Import(name, span));
            }
            _ => match self.decl(node)? {
                Some(decl) => items.push(Item::Decl(decl)),
                None => items.push(Item::Stmt(self.stmt(node)?)),
            },
        }
        Ok(())
    }

    /// `KEYWORD name`, as in `NAMESPACE fd` and `IMPORT git`.
    fn single_name(&self, flat: &[Flat], msg: &str) -> Res<String> {
        match flat {
            [_, Flat::Word(name, _)] if !is_symbol(name) => Ok(name.to_string()),
            [keyword, ..] => self.err(flat_span(keyword), format!("{msg}; {SEPARATE}")),
            [] => unreachable!("a call has a name"),
        }
    }

    // ---------- declarations ----------

    fn decl(&self, node: &Node) -> Res<Option<Decl>> {
        let flat = flatten(node);
        let doc = self.docs.get(&self.at(node.span).line).cloned();
        // `FUNCTION sig -> Ret { body }` and `TYPE pattern = target` are split
        // by their symbol word, so the keyword heads the left side.
        if let [Flat::Arg(left), Flat::Word(symbol, _), Flat::Arg(right)] = flat.as_slice() {
            let left_flat = flatten(left);
            match (head(&left_flat), *symbol) {
                (Some("FUNCTION"), "->") => {
                    let right_flat = flatten(right);
                    let (body, ret) =
                        self.trailing_braces(&right_flat, right.span, "function body")?;
                    let ret = self.ty_flat(ret, right.span)?;
                    return self
                        .function(&left_flat[1..], Some(ret), body, node, doc)
                        .map(Some);
                }
                (Some("TYPE"), "=") => {
                    let pattern = self.ty_flat(&left_flat[1..], left.span)?;
                    let target = self.ty(right)?;
                    return Ok(Some(Decl::Alias {
                        pattern,
                        target,
                        span: self.at(node.span),
                    }));
                }
                (Some(keyword @ ("FUNCTION" | "TYPE")), _) => {
                    return self.err(
                        node.span,
                        format!(
                            "`{keyword}` is written {}",
                            if keyword == "TYPE" {
                                "`TYPE Name (T) = Target`"
                            } else {
                                "`FUNCTION signature -> Return Type { body }`"
                            }
                        ),
                    );
                }
                _ => {}
            }
        }
        if let [Flat::Arg(left), Flat::Word("->", _), ..] = flat.as_slice()
            && head(&flatten(left)) == Some("FUNCTION")
        {
            return self.err(
                node.span,
                format!("expected one `FUNCTION signature -> Return Type {{ body }}`; is a `,` missing after a `}}`? ({SEPARATE})"),
            );
        }
        match head(&flat) {
            Some("FUNCTION") => {
                let (body, sig) = self.trailing_braces(&flat[1..], node.span, "function body")?;
                self.function(sig, None, body, node, doc).map(Some)
            }
            Some("RECORD") => {
                let (body, name) =
                    self.trailing_braces(&flat[1..], node.span, "record's fields")?;
                let fields = body.iter().map(|f| self.field(f)).collect::<Res<_>>()?;
                Ok(Some(Decl::Record {
                    name: self.decl_name(name, node.span)?,
                    fields,
                    span: self.at(node.span),
                    doc,
                }))
            }
            Some("ENUM") => {
                let (body, name) =
                    self.trailing_braces(&flat[1..], node.span, "enum's variants")?;
                let variants = body
                    .iter()
                    .map(|v| self.signature(&flatten(v), true, v.span))
                    .collect::<Res<_>>()?;
                Ok(Some(Decl::Enum {
                    name: self.decl_name(name, node.span)?,
                    variants,
                    span: self.at(node.span),
                    doc,
                }))
            }
            Some("TYPE") => self.err(node.span, "`TYPE` is written `TYPE Name (T) = Target`"),
            _ => Ok(None),
        }
    }

    /// Splits a declaration's trailing `{ ... }` off the items before it.
    fn trailing_braces<'a>(
        &self,
        items: &'a [Flat<'a>],
        span: anyfix_expr::Span,
        what: &str,
    ) -> Res<(&'a [Node], &'a [Flat<'a>])> {
        let Some(position) = items
            .iter()
            .position(|f| matches!(f, Flat::Arg(n) if braces(n).is_some()))
        else {
            return self.err(span, format!("expected `{{ ... }}` with the {what}"));
        };
        if let Some(after) = items.get(position + 1) {
            return self.err(
                flat_span(after),
                format!("expected `,` after the {what}'s `}}`; {SEPARATE}"),
            );
        }
        let Flat::Arg(node) = items[position] else {
            unreachable!()
        };
        Ok((braces(node).expect("checked above"), &items[..position]))
    }

    fn function(
        &self,
        sig: &[Flat],
        ret: Option<TyExpr>,
        body: &[Node],
        node: &Node,
        doc: Option<String>,
    ) -> Res<Decl> {
        let sig = self.signature(sig, false, node.span)?;
        let body = match body {
            [native] => match flatten(native).as_slice() {
                [Flat::Word("NATIVE", _), Flat::Arg(name)] => match &name.kind {
                    NodeKind::String(name) => FnBody::Native(name.clone()),
                    _ => return self.err(name.span, "expected a native function name string"),
                },
                _ => FnBody::Block(self.block(body)?),
            },
            _ => FnBody::Block(self.block(body)?),
        };
        Ok(Decl::Function {
            sig,
            ret,
            body,
            doc,
        })
    }

    fn decl_name(&self, items: &[Flat], span: anyfix_expr::Span) -> Res<String> {
        let mut words = Vec::new();
        for item in items {
            match item {
                Flat::Word(w, s) => {
                    self.check_capitalized(w, *s)?;
                    words.push(*w);
                }
                Flat::Arg(n) => return self.err(n.span, "a declared name has no parameters"),
            }
        }
        if words.is_empty() {
            return self.err(span, "expected a name");
        }
        Ok(words.join(" "))
    }

    /// `bare_types` (enum variants): a slot may hold a type alone,
    /// `max depth (Int)`; it gets the placeholder name `_`.
    fn signature(&self, items: &[Flat], bare_types: bool, span: anyfix_expr::Span) -> Res<SigAst> {
        let parts = items
            .iter()
            .map(|item| match item {
                Flat::Word(w, s) if is_symbol(w) => self.err(
                    *s,
                    format!(
                        "`{w}` cannot be part of a name; {SEPARATE}, and a return type follows `->`"
                    ),
                ),
                Flat::Word(w, _) => Ok(Part::Word(w.to_string())),
                Flat::Arg(n) => self.param(n, bare_types),
            })
            .collect::<Res<_>>()?;
        Ok(SigAst {
            parts,
            span: self.at(span),
        })
    }

    /// `(name : Type)`, a tuple `((name : Type, name : Type))`, or (in a
    /// variant) a bare type.
    fn param(&self, node: &Node, bare_types: bool) -> Res<Part> {
        if let NodeKind::Parens(components) = &node.kind {
            let mut out = Vec::new();
            for c in components {
                match flatten(c).as_slice() {
                    [Flat::Word(name, _), Flat::Arg(ty)] if is_lowercase_word(name) => {
                        out.push((name.to_string(), self.ty(ty)?))
                    }
                    _ => return self.err(c.span, "expected a component `name : Type`"),
                }
            }
            if out.len() < 2 {
                return self.err(
                    node.span,
                    "a tuple parameter has two or more components, e.g. `((command : String, args : List (String)))`",
                );
            }
            return Ok(Part::Tuple(out));
        }
        match flatten(node).as_slice() {
            [Flat::Word(name, _), Flat::Arg(ty)] if is_lowercase_word(name) => Ok(Part::Param {
                name: name.to_string(),
                ty: Some(self.ty(ty)?),
            }),
            [Flat::Word(name, _)] if is_lowercase_word(name) && !bare_types => Ok(Part::Param {
                name: name.to_string(),
                ty: None,
            }),
            _ if bare_types => Ok(Part::Param {
                name: "_".into(),
                ty: Some(self.ty(node)?),
            }),
            _ => self.err(node.span, "expected a parameter `(name : Type)`"),
        }
    }

    /// A record field: words and type-only slots, each slot optionally
    /// `DEFAULT value`, e.g. `port (Int DEFAULT 8080)`.
    fn field(&self, node: &Node) -> Res<FieldAst> {
        let mut parts = Vec::new();
        for item in flatten(node) {
            match item {
                Flat::Word(w, s) if is_symbol(w) => {
                    return self.err(s, format!("`{w}` cannot be part of a field name"));
                }
                Flat::Word(w, s) if !is_lowercase_word(w) => {
                    return self.err(
                        s,
                        format!("field names are lowercase; `{w}` looks like a type, which goes in a slot: `name ({w})` (a field needs at least one word)"),
                    );
                }
                Flat::Word(w, _) => parts.push(FieldPart::Word(w.to_string())),
                Flat::Arg(slot) => {
                    let inner = flatten(slot);
                    let (ty, default) = match inner
                        .iter()
                        .position(|f| matches!(f, Flat::Word("DEFAULT", _)))
                    {
                        Some(i) => match &inner[i + 1..] {
                            [Flat::Arg(value)] => (&inner[..i], Some(self.expr(value)?)),
                            _ => {
                                return self.err(
                                    slot.span,
                                    "expected one value after `DEFAULT`: `(Type DEFAULT value)`, with a longer value in parentheses",
                                );
                            }
                        },
                        None => (&inner[..], None),
                    };
                    parts.push(FieldPart::Slot {
                        ty: self.ty_flat(ty, slot.span)?,
                        default,
                    });
                }
            }
        }
        if !parts.iter().any(|p| matches!(p, FieldPart::Word(_))) {
            return self.err(
                node.span,
                "expected a field name (a field needs at least one word)",
            );
        }
        Ok(FieldAst {
            parts,
            span: self.at(node.span),
        })
    }

    // ---------- types ----------

    fn ty(&self, node: &Node) -> Res<TyExpr> {
        self.ty_flat(&flatten(node), node.span)
    }

    /// An anyfix type: capitalized words and `(Type)` slots, e.g.
    /// `Result (List (Entry)) Or (String)`, optionally qualified: `git.Commit`.
    /// `name` keeps the words with `_` standing in for each slot (`Result _ Or _`).
    fn ty_flat(&self, items: &[Flat], span: anyfix_expr::Span) -> Res<TyExpr> {
        // `-> (git.Commit) { ... }`: parentheses keep a qualified type whole.
        if let [Flat::Arg(inner)] = items
            && matches!(inner.kind, NodeKind::Call(_))
        {
            return self.ty(inner);
        }
        let (qualifiers, items) = match items {
            [Flat::Arg(q), rest @ ..]
                if !rest.is_empty() && single_word(q).is_some_and(is_lowercase_word) =>
            {
                (vec![single_word(q).unwrap_or_default().to_string()], rest)
            }
            _ => (Vec::new(), items),
        };
        let mut names = Vec::new();
        let mut args = Vec::new();
        for item in items {
            match item {
                Flat::Word(w, s) if is_symbol(w) => {
                    return self.err(
                        *s,
                        format!("`{w}` cannot be part of a type; type parameters go in parentheses: `List (Int)`"),
                    );
                }
                Flat::Word(w, s) => {
                    self.check_capitalized(w, *s)?;
                    names.push(w.to_string());
                }
                Flat::Arg(n) => {
                    names.push("_".into());
                    args.push(self.ty(n)?);
                }
            }
        }
        if !items.iter().any(|f| matches!(f, Flat::Word(..))) {
            return self.err(span, "expected a type");
        }
        Ok(TyExpr {
            qualifiers,
            name: names.join(" "),
            args,
            span: self.at(span),
        })
    }

    fn check_capitalized(&self, w: &str, span: anyfix_expr::Span) -> Res<()> {
        if w.starts_with(|c: char| c.is_uppercase()) {
            Ok(())
        } else {
            self.err(
                span,
                format!(
                    "type names capitalize every word; write `{}` here",
                    capitalize(w)
                ),
            )
        }
    }

    // ---------- statements ----------

    fn block(&self, nodes: &[Node]) -> Res<Vec<Stmt>> {
        nodes.iter().map(|n| self.stmt(n)).collect()
    }

    fn stmt(&self, node: &Node) -> Res<Stmt> {
        let flat = flatten(node);
        let span = self.at(node.span);
        match flat.as_slice() {
            [Flat::Word("LET", _), rest @ ..] => match rest {
                [Flat::Word(name, _), Flat::Arg(value)] if !is_symbol(name) => {
                    Ok(Stmt::Let(name.to_string(), self.expr(value)?, span))
                }
                [Flat::Arg(binding), Flat::Arg(value)] => match flatten(binding).as_slice() {
                    [Flat::Word(name, _), Flat::Arg(ty)] if !is_symbol(name) => {
                        let value = Expr {
                            k: ExprKind::Ascribe(Box::new(self.expr(value)?), self.ty(ty)?),
                            span,
                        };
                        Ok(Stmt::Let(name.to_string(), value, span))
                    }
                    _ => self.let_shape(node),
                },
                _ => self.let_shape(node),
            },
            [Flat::Arg(ns), Flat::Arg(body)] if braces(body).is_some() => match single_word(ns) {
                Some(ns) => Ok(Stmt::Scope(
                    ns.to_string(),
                    self.block(braces(body).expect("checked above"))?,
                    span,
                )),
                None => Ok(Stmt::Expr(self.expr(node)?)),
            },
            [Flat::Arg(left), Flat::Word("=", _), ..] if head(&flatten(left)) == Some("LET") => {
                self.let_shape(node)
            }
            _ => {
                if let Some(keyword) = head(&flat).filter(|k| {
                    ["FUNCTION", "RECORD", "ENUM", "TYPE", "NAMESPACE", "IMPORT"].contains(k)
                }) {
                    return self.err(
                        node.span,
                        format!(
                            "`{keyword}` declares at the top level of a file, not inside a block"
                        ),
                    );
                }
                Ok(Stmt::Expr(self.expr(node)?))
            }
        }
    }

    fn let_shape<T>(&self, node: &Node) -> Res<T> {
        self.err(
            node.span,
            "`LET` is written `LET name : value` or `LET (name : Type) : value`; a variable name is a single word",
        )
    }

    // ---------- expressions ----------

    fn expr(&self, node: &Node) -> Res<Expr> {
        let span = self.at(node.span);
        let k = match &node.kind {
            NodeKind::Number(text) => self.number(text, node.span)?,
            NodeKind::String(s) => ExprKind::Str(s.clone()),
            NodeKind::Underscore => ExprKind::Placeholder,
            NodeKind::Missing => return self.err(node.span, "expected an expression"),
            NodeKind::Parens(xs) => match xs.as_slice() {
                [] => return self.err(node.span, "expected an expression inside `()`"),
                [x] if matches!(x.kind, NodeKind::Underscore) => ExprKind::Placeholder,
                [_] => {
                    return self.err(
                        node.span,
                        "a tuple has two or more values; remove the trailing `,`",
                    );
                }
                _ => ExprKind::Tuple(xs.iter().map(|x| self.expr(x)).collect::<Res<_>>()?),
            },
            NodeKind::Brackets(xs) => {
                ExprKind::List(xs.iter().map(|x| self.expr(x)).collect::<Res<_>>()?)
            }
            NodeKind::Braces(entries) => ExprKind::Brace(
                entries
                    .iter()
                    .map(|entry| {
                        Ok(BraceEntry {
                            items: flatten(entry)
                                .iter()
                                .map(|f| self.seq_item(f))
                                .collect::<Res<_>>()?,
                            span: self.at(entry.span),
                        })
                    })
                    .collect::<Res<_>>()?,
            ),
            NodeKind::Call(_) => return self.flat_expr(&flatten(node)),
        };
        Ok(Expr { k, span })
    }

    fn number(&self, text: &str, span: anyfix_expr::Span) -> Res<ExprKind> {
        if let Ok(n) = text.parse::<i64>() {
            return Ok(ExprKind::Int(n));
        }
        if text.contains('.')
            && let Ok(f) = text.parse::<f64>()
        {
            return Ok(ExprKind::Float(f));
        }
        if text.chars().all(|c| c.is_ascii_digit()) {
            return self.err(span, "integer literal out of range");
        }
        let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
        let suffix = &text[digits.len()..];
        self.err(
            span,
            format!("invalid number `{text}`; a unit is a separate word: `{digits} {suffix}`"),
        )
    }

    fn seq_item(&self, item: &Flat) -> Res<SeqItem> {
        match item {
            Flat::Word(w, s) => Ok(SeqItem::Word(w.to_string(), self.at(*s))),
            Flat::Arg(n) => Ok(SeqItem::Atom(self.expr(n)?)),
        }
    }

    /// The items of one call: a keyword form (`IF`, `MATCH`, `AS`, a lambda,
    /// a spread) or a plain `Seq` for the checker to resolve.
    fn flat_expr(&self, items: &[Flat]) -> Res<Expr> {
        let first = flat_span(&items[0]);
        let span = self.at(first);
        let mk = |k: ExprKind| Ok(Expr { k, span });
        let word_at = |word: &str| {
            items
                .iter()
                .position(|f| matches!(f, Flat::Word(w, _) if *w == word))
        };
        match items {
            [Flat::Arg(n)] => return self.expr(n),
            [
                Flat::Word("IF", _),
                Flat::Arg(cond),
                Flat::Word("THEN", _),
                Flat::Arg(yes),
                Flat::Word("ELSE", _),
                Flat::Arg(no),
            ] => {
                return mk(ExprKind::If(
                    Box::new(self.expr(cond)?),
                    Box::new(self.expr(yes)?),
                    Box::new(self.expr(no)?),
                ));
            }
            [Flat::Arg(param), Flat::Word("=>", _), Flat::Arg(body)] => {
                let Some(name) = single_word(param) else {
                    return self.err(
                        param.span,
                        "a lambda is written `(name) => body`: exactly one parameter name in parentheses",
                    );
                };
                return mk(ExprKind::Lambda(
                    name.to_string(),
                    Box::new(self.expr(body)?),
                ));
            }
            [Flat::Word("*", _), Flat::Arg(list)] => {
                return mk(ExprKind::Spread(Box::new(self.expr(list)?)));
            }
            _ => {}
        }
        match items {
            [Flat::Word("MATCH", _), Flat::Arg(subject), Flat::Arg(arms)]
            | [Flat::Arg(subject), Flat::Word("MATCH", _), Flat::Arg(arms)] => {
                return self.match_expr(subject, arms, first);
            }
            [Flat::Arg(value), Flat::Word("AS", _), Flat::Arg(ty)] => {
                return mk(ExprKind::Ascribe(Box::new(self.expr(value)?), self.ty(ty)?));
            }
            _ => {}
        }
        for (keyword, shape) in [
            ("IF", "`IF (condition) THEN (value) ELSE (value)`"),
            ("THEN", "`IF (condition) THEN (value) ELSE (value)`"),
            ("ELSE", "`IF (condition) THEN (value) ELSE (value)`"),
            (
                "MATCH",
                "`MATCH (value) { arms }` or `value . MATCH { arms }`",
            ),
            ("AS", "`(value) AS (Type)`"),
        ] {
            if let Some(i) = word_at(keyword) {
                return self.err(
                    flat_span(&items[i]),
                    format!(
                        "`{keyword}` is part of the call {shape}: each part is one item, so a longer expression goes in parentheses (or a `,` is missing before it)"
                    ),
                );
            }
        }
        for item in items {
            if let Flat::Word(w, s) = item {
                if KEYWORDS.contains(w) {
                    return self.err(
                        *s,
                        format!(
                            "`{w}` cannot appear in the middle of a call; is a `,` missing before it? ({SEPARATE})"
                        ),
                    );
                }
                if *w == ";" {
                    return self.err(*s, format!("`;` does not end a statement; {SEPARATE}"));
                }
                if is_symbol(w) {
                    return self.err(
                        *s,
                        format!(
                            "`{w}` is not an operator in resh; `.` and `:` chain calls, `=>` writes a lambda or a MATCH arm, `->` a return type"
                        ),
                    );
                }
            }
        }
        mk(ExprKind::Seq(
            items.iter().map(|f| self.seq_item(f)).collect::<Res<_>>()?,
        ))
    }

    /// `MATCH (subject) { arms }` or `subject . MATCH { arms }`.
    fn match_expr(&self, subject: &Node, arms_node: &Node, span: anyfix_expr::Span) -> Res<Expr> {
        if braces(arms_node).is_none() {
            return self.err(
                arms_node.span,
                "expected `{ arms }` at the end of the MATCH",
            );
        }
        let subject = self.expr(subject)?;
        let mut arms = Vec::new();
        for arm in braces(arms_node).expect("checked above") {
            match flatten(arm).as_slice() {
                [Flat::Arg(pat), Flat::Word("=>", _), Flat::Arg(body)] => arms.push(Arm {
                    pat: self.pattern(pat)?,
                    body: self.expr(body)?,
                    span: self.at(arm.span),
                }),
                _ => return self.err(arm.span, "expected a MATCH arm `pattern => value`"),
            }
        }
        Ok(Expr {
            k: ExprKind::Match(Box::new(subject), arms),
            span: self.at(span),
        })
    }

    /// A MATCH pattern: words, `(name)` binders, `_`, or a `{ … }` record pattern.
    fn pattern(&self, node: &Node) -> Res<Vec<PatItem>> {
        match &node.kind {
            NodeKind::Underscore => Ok(vec![PatItem::Wild]),
            NodeKind::Braces(fields) => Ok(vec![PatItem::Record(
                fields.iter().map(|f| self.pattern(f)).collect::<Res<_>>()?,
            )]),
            NodeKind::Call(_) => flatten(node)
                .iter()
                .map(|item| match item {
                    Flat::Word(w, s) if is_symbol(w) => {
                        self.err(*s, format!("`{w}` cannot appear in a pattern"))
                    }
                    Flat::Word(w, _) => Ok(PatItem::Word(w.to_string())),
                    Flat::Arg(slot) => self.pattern_slot(slot),
                })
                .collect(),
            _ => self.err(
                node.span,
                "expected a pattern: variant words, `(name)` binders, `_` or `{ ... }`",
            ),
        }
    }

    fn pattern_slot(&self, slot: &Node) -> Res<PatItem> {
        match &slot.kind {
            NodeKind::Underscore => Ok(PatItem::Wild),
            NodeKind::Parens(xs) if matches!(xs.as_slice(), [x] if matches!(x.kind, NodeKind::Underscore)) => {
                Ok(PatItem::Bind("_".into()))
            }
            NodeKind::Braces(_) => Ok(PatItem::Nested(self.pattern(slot)?)),
            NodeKind::Call(_) => {
                let mut inner = self.pattern(slot)?;
                match inner.as_mut_slice() {
                    [PatItem::Word(w)] => Ok(PatItem::Bind(std::mem::take(w))),
                    _ => Ok(PatItem::Nested(inner)),
                }
            }
            _ => self.err(
                slot.span,
                "a pattern slot holds a binder `(name)`, `(_)` or a nested pattern",
            ),
        }
    }
}
