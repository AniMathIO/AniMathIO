//! A small, pure-Rust concrete syntax tree for the Python subset Manim scenes use.
//!
//! The importer used tree-sitter-python, whose C runtime cannot be built for the
//! `wasm32-wasip2` importer plugin without a WASI C toolchain. This module keeps the
//! same node vocabulary (kind and field names follow tree-sitter-python) and a
//! compatible read-only API, so [`crate::ast`] and [`crate::eval`] walk the tree as
//! before. Syntax errors become `ERROR` nodes spanning the failed statement and
//! parsing resumes at the next statement, so one bad line never fails an import.
//! Expressions nested deeper than [`MAX_NESTING`] become a [`TOO_DEEP`] node instead
//! of recursing further, which keeps the parser's stack bounded on hostile input.

/// Kind of a statement whose expression nesting exceeded [`MAX_NESTING`].
pub(crate) const TOO_DEEP: &str = "too_deep";
/// Deepest bracket/unary nesting parsed recursively (tree-sitter depth > 64 was
/// already rejected by the importer, so nothing legitimate is lost).
const MAX_NESTING: usize = 80;

/// Zero-based row and byte column, like `tree_sitter::Point`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Point {
    pub row: usize,
    pub column: usize,
}

struct NodeData {
    kind: &'static str,
    named: bool,
    error: bool,
    start: usize,
    end: usize,
    children: Vec<(Option<&'static str>, usize)>,
}

/// A parsed module.
pub(crate) struct Tree {
    nodes: Vec<NodeData>,
    line_starts: Vec<usize>,
    root: usize,
}

impl Tree {
    pub(crate) fn root_node(&self) -> Node<'_> {
        Node {
            tree: self,
            id: self.root,
        }
    }
}

/// Stand-in for `tree_sitter::TreeCursor`; iteration needs no state here.
pub(crate) struct Cursor;

/// A node handle; cheap to copy.
#[derive(Clone, Copy)]
pub(crate) struct Node<'t> {
    tree: &'t Tree,
    id: usize,
}

impl std::fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.kind(), self.data().start)
    }
}

impl<'t> Node<'t> {
    fn data(&self) -> &'t NodeData {
        &self.tree.nodes[self.id]
    }
    fn at(&self, id: usize) -> Node<'t> {
        Node {
            tree: self.tree,
            id,
        }
    }
    pub(crate) fn kind(&self) -> &'static str {
        self.data().kind
    }
    pub(crate) fn is_named(&self) -> bool {
        self.data().named
    }
    pub(crate) fn is_error(&self) -> bool {
        self.data().error
    }
    /// Missing-token nodes are never produced; the failed statement is an `ERROR`.
    #[allow(clippy::unused_self)]
    pub(crate) fn is_missing(&self) -> bool {
        false
    }
    #[allow(clippy::unused_self)] // Mirrors `tree_sitter::Node::walk`.
    pub(crate) fn walk(&self) -> Cursor {
        Cursor
    }
    pub(crate) fn start_position(&self) -> Point {
        let start = self.data().start;
        let row = self
            .tree
            .line_starts
            .partition_point(|&line| line <= start)
            .saturating_sub(1);
        Point {
            row,
            column: start - self.tree.line_starts[row],
        }
    }
    pub(crate) fn utf8_text<'s>(&self, source: &'s [u8]) -> Result<&'s str, std::str::Utf8Error> {
        let data = self.data();
        std::str::from_utf8(&source[data.start..data.end])
    }
    pub(crate) fn child_by_field_name(&self, field: &str) -> Option<Node<'t>> {
        self.data()
            .children
            .iter()
            .find(|(name, _)| *name == Some(field))
            .map(|(_, id)| self.at(*id))
    }
    pub(crate) fn named_child(&self, index: usize) -> Option<Node<'t>> {
        self.data()
            .children
            .iter()
            .map(|(_, id)| self.at(*id))
            .filter(Node::is_named)
            .nth(index)
    }
    pub(crate) fn named_children(&self, _cursor: &mut Cursor) -> std::vec::IntoIter<Node<'t>> {
        self.data()
            .children
            .iter()
            .map(|(_, id)| self.at(*id))
            .filter(Node::is_named)
            .collect::<Vec<_>>()
            .into_iter()
    }
    pub(crate) fn children(&self, _cursor: &mut Cursor) -> std::vec::IntoIter<Node<'t>> {
        self.data()
            .children
            .iter()
            .map(|(_, id)| self.at(*id))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Name,
    Number,
    Str,
    Op,
    Newline,
    Indent,
    Dedent,
    /// Unrecognised input, an unterminated string or inconsistent indentation.
    Error,
    End,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Tok,
    start: usize,
    end: usize,
}

const OPERATORS: [&str; 48] = [
    "**=", "//=", ">>=", "<<=", "...", "!=", "%=", "&=", "**", "*=", "+=", "-=", "->", "//", "/=",
    ":=", "<<", "<=", "==", ">=", ">>", "@=", "^=", "|=", "%", "&", "(", ")", "*", "+", ",", "-",
    ".", "/", ":", ";", "<", "=", ">", "@", "[", "]", "^", "{", "|", "}", "~", "!",
];

/// Keywords that can only begin a statement: seeing one at the start of a line
/// whose indentation does not exceed the line that opened an unclosed bracket ends
/// that bracket, so a missing `)` does not swallow the rest of the file.
const STATEMENT_KEYWORDS: [&str; 16] = [
    "def", "class", "return", "import", "pass", "while", "with", "try", "except", "finally",
    "elif", "raise", "del", "global", "nonlocal", "assert",
];

fn is_name_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_name_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

struct Lexer<'s> {
    src: &'s str,
    pos: usize,
    tokens: Vec<Token>,
    indents: Vec<usize>,
    /// Indentation of the line that opened each unclosed bracket.
    brackets: Vec<usize>,
    line_has_tokens: bool,
}

impl Lexer<'_> {
    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }
    fn push(&mut self, kind: Tok, start: usize, end: usize) {
        self.tokens.push(Token { kind, start, end });
        if !matches!(kind, Tok::Newline | Tok::Indent | Tok::Dedent) {
            self.line_has_tokens = true;
        }
    }

    fn run(mut self) -> Vec<Token> {
        let mut at_line_start = true;
        while self.pos < self.src.len() {
            if at_line_start {
                at_line_start = false;
                self.line_start();
                continue;
            }
            let Some(c) = self.peek_char() else { break };
            let start = self.pos;
            match c {
                ' ' | '\t' | '\x0c' | '\r' => self.pos += 1,
                '#' => {
                    while self.peek_char().is_some_and(|c| c != '\n') {
                        self.pos += self.peek_char().map_or(1, char::len_utf8);
                    }
                }
                '\\' if self.src[self.pos + 1..].starts_with('\n')
                    || self.src[self.pos + 1..].starts_with("\r\n") =>
                {
                    self.pos += if self.src[self.pos + 1..].starts_with('\n') {
                        2
                    } else {
                        3
                    };
                }
                '\n' => {
                    self.pos += 1;
                    if self.brackets.is_empty() {
                        if self.line_has_tokens {
                            self.push(Tok::Newline, start, start);
                            self.line_has_tokens = false;
                        }
                        at_line_start = true;
                    } else {
                        self.continuation_line();
                    }
                }
                '"' | '\'' => self.string(start),
                c if c.is_ascii_digit()
                    || (c == '.'
                        && self.src[self.pos + 1..]
                            .chars()
                            .next()
                            .is_some_and(|d| d.is_ascii_digit())) =>
                {
                    self.number(start);
                }
                c if is_name_start(c) => {
                    while self.peek_char().is_some_and(is_name_char) {
                        self.pos += self.peek_char().map_or(1, char::len_utf8);
                    }
                    let word = &self.src[start..self.pos];
                    if is_string_prefix(word) && matches!(self.peek_char(), Some('"' | '\'')) {
                        self.string(start);
                    } else {
                        self.push(Tok::Name, start, self.pos);
                    }
                }
                _ => {
                    let rest = &self.src[self.pos..];
                    if let Some(op) = OPERATORS.iter().find(|op| rest.starts_with(**op)) {
                        self.pos += op.len();
                        match *op {
                            "(" | "[" | "{" => {
                                let indent = self.current_indent();
                                self.brackets.push(indent);
                            }
                            ")" | "]" | "}" => {
                                self.brackets.pop();
                            }
                            _ => {}
                        }
                        let kind = if *op == "!" { Tok::Error } else { Tok::Op };
                        self.push(kind, start, self.pos);
                    } else {
                        self.pos += c.len_utf8();
                        self.push(Tok::Error, start, self.pos);
                    }
                }
            }
        }
        if self.line_has_tokens {
            self.push(Tok::Newline, self.src.len(), self.src.len());
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            self.push(Tok::Dedent, self.src.len(), self.src.len());
        }
        self.push(Tok::End, self.src.len(), self.src.len());
        self.tokens
    }

    fn current_indent(&self) -> usize {
        let line_start = self.src[..self.pos].rfind('\n').map_or(0, |i| i + 1);
        measure_indent(&self.src[line_start..])
    }

    /// Inside brackets a newline continues the logical line unless the next line
    /// clearly starts a new statement at the bracket's indentation or less.
    fn continuation_line(&mut self) {
        let line = &self.src[self.pos..];
        let indent = measure_indent(line);
        let content = line.trim_start_matches([' ', '\t', '\x0c']);
        let word: String = content.chars().take_while(|c| is_name_char(*c)).collect();
        let opened = self.brackets.first().copied().unwrap_or(0);
        if indent <= opened && STATEMENT_KEYWORDS.contains(&word.as_str()) {
            // Unclosed bracket: flag it, then resume indentation tracking.
            self.push(Tok::Error, self.pos, self.pos);
            self.brackets.clear();
            self.push(Tok::Newline, self.pos, self.pos);
            self.line_has_tokens = false;
            self.line_start();
        }
    }

    /// Measure indentation at the start of a physical line and emit
    /// Indent/Dedent tokens; blank and comment-only lines are skipped.
    fn line_start(&mut self) {
        loop {
            let line = &self.src[self.pos..];
            let indent = measure_indent(line);
            let content_at = line.len() - line.trim_start_matches([' ', '\t', '\x0c']).len();
            let rest = &line[content_at..];
            if rest.is_empty() {
                self.pos = self.src.len();
                return;
            }
            if rest.starts_with('\n') || rest.starts_with("\r\n") || rest.starts_with('#') {
                // Skip the whole blank or comment line.
                if let Some(newline) = line.find('\n') {
                    self.pos += newline + 1;
                } else {
                    self.pos = self.src.len();
                    return;
                }
                continue;
            }
            self.pos += content_at;
            let current = *self.indents.last().unwrap_or(&0);
            if indent > current {
                self.indents.push(indent);
                self.push(Tok::Indent, self.pos, self.pos);
            } else if indent < current {
                while self.indents.last().is_some_and(|&top| top > indent) {
                    self.indents.pop();
                    self.push(Tok::Dedent, self.pos, self.pos);
                }
                if self.indents.last().copied().unwrap_or(0) != indent {
                    // Dedent to a level that was never opened.
                    self.indents.push(indent);
                    self.push(Tok::Error, self.pos, self.pos);
                }
            }
            self.line_has_tokens = false;
            return;
        }
    }

    fn number(&mut self, start: usize) {
        let bytes = self.src.as_bytes();
        let mut i = self.pos;
        let mut prev = 0u8;
        while i < bytes.len() {
            let b = bytes[i];
            let exponent_sign = matches!(b, b'+' | b'-') && matches!(prev, b'e' | b'E');
            let hex = self.src[start..].starts_with("0x") || self.src[start..].starts_with("0X");
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || (exponent_sign && !hex) {
                prev = b;
                i += 1;
            } else {
                break;
            }
        }
        self.pos = i;
        self.push(Tok::Number, start, i);
    }

    fn string(&mut self, start: usize) {
        let quote = self.peek_char().unwrap_or('"');
        let triple: String = std::iter::repeat_n(quote, 3).collect();
        let is_triple = self.src[self.pos..].starts_with(&triple);
        self.pos += if is_triple { 3 } else { 1 };
        loop {
            let Some(c) = self.peek_char() else {
                // Unterminated at end of input.
                self.push(Tok::Error, start, self.pos);
                return;
            };
            if c == '\\' {
                self.pos += 1;
                self.pos += self.peek_char().map_or(0, char::len_utf8);
                continue;
            }
            if is_triple {
                if self.src[self.pos..].starts_with(&triple) {
                    self.pos += 3;
                    break;
                }
            } else if c == quote {
                self.pos += 1;
                break;
            } else if c == '\n' {
                self.push(Tok::Error, start, self.pos);
                return;
            }
            self.pos += c.len_utf8();
        }
        self.push(Tok::Str, start, self.pos);
    }
}

fn measure_indent(line: &str) -> usize {
    let mut width = 0;
    for c in line.chars() {
        match c {
            ' ' => width += 1,
            '\t' => width = (width / 8 + 1) * 8,
            '\x0c' => width = 0,
            _ => break,
        }
    }
    width
}

fn is_string_prefix(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "r" | "u" | "b" | "f" | "br" | "rb" | "fr" | "rf"
    )
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Fail {
    Syntax,
    TooDeep,
}

type Parsed = Result<usize, Fail>;

struct Parser<'s> {
    src: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    nodes: Vec<NodeData>,
    depth: usize,
}

const COMPARISONS: [&str; 6] = ["<", ">", "==", ">=", "<=", "!="];
const AUGMENTED: [&str; 12] = [
    "+=", "-=", "*=", "/=", "//=", "%=", "**=", ">>=", "<<=", "&=", "^=", "|=",
];

impl<'s> Parser<'s> {
    fn tok(&self) -> Token {
        self.tokens[self.pos.min(self.tokens.len() - 1)]
    }
    fn text(&self, token: Token) -> &'s str {
        &self.src[token.start..token.end]
    }
    fn at_op(&self, op: &str) -> bool {
        let t = self.tok();
        t.kind == Tok::Op && self.text(t) == op
    }
    fn at_keyword(&self, word: &str) -> bool {
        let t = self.tok();
        t.kind == Tok::Name && self.text(t) == word
    }
    fn bump(&mut self) -> Token {
        let t = self.tok();
        if t.kind != Tok::End {
            self.pos += 1;
        }
        t
    }
    fn expect_op(&mut self, op: &str) -> Result<Token, Fail> {
        if self.at_op(op) {
            Ok(self.bump())
        } else {
            Err(Fail::Syntax)
        }
    }
    fn prev_end(&self) -> usize {
        self.tokens[self.pos.saturating_sub(1)].end
    }

    fn node(
        &mut self,
        kind: &'static str,
        start: usize,
        end: usize,
        children: Vec<(Option<&'static str>, usize)>,
    ) -> usize {
        self.nodes.push(NodeData {
            kind,
            named: true,
            error: false,
            start,
            end,
            children,
        });
        self.nodes.len() - 1
    }
    fn anon(&mut self, token: Token) -> usize {
        self.nodes.push(NodeData {
            kind: "operator",
            named: false,
            error: false,
            start: token.start,
            end: token.end,
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }
    fn span(&self, id: usize) -> (usize, usize) {
        (self.nodes[id].start, self.nodes[id].end)
    }
    fn descend(&mut self) -> Result<(), Fail> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            Err(Fail::TooDeep)
        } else {
            Ok(())
        }
    }

    // --- statements -------------------------------------------------------

    fn module(&mut self) -> usize {
        let mut children = Vec::new();
        while self.tok().kind != Tok::End {
            if matches!(self.tok().kind, Tok::Newline | Tok::Dedent | Tok::Indent) {
                // Stray layout tokens at top level: an unexpected indent is an error.
                let t = self.bump();
                if t.kind == Tok::Indent {
                    let id = self.recover(t.start, Fail::Syntax);
                    children.push((None, id));
                }
                continue;
            }
            children.extend(self.statement().into_iter().map(|id| (None, id)));
        }
        self.node("module", 0, self.src.len(), children)
    }

    /// One logical statement (a simple line may hold several `;`-separated ones).
    fn statement(&mut self) -> Vec<usize> {
        let start_pos = self.pos;
        let start = self.tok().start;
        // Nesting counts across statements: a block inside a block is deeper.
        let saved = self.depth;
        let result = self.statement_inner();
        self.depth = saved;
        match result {
            Ok(ids) => ids,
            Err(fail) => {
                self.pos = start_pos;
                vec![self.recover(start, fail)]
            }
        }
    }

    /// Skip to the end of the logical line (plus any block it opens) and record an
    /// `ERROR` (or [`TOO_DEEP`]) node over the skipped text.
    fn recover(&mut self, start: usize, fail: Fail) -> usize {
        let mut end = start;
        loop {
            let t = self.tok();
            match t.kind {
                Tok::End | Tok::Dedent => break,
                Tok::Newline => {
                    self.bump();
                    break;
                }
                _ => {
                    end = end.max(t.end);
                    self.bump();
                }
            }
        }
        if self.tok().kind == Tok::Indent {
            let mut level = 0usize;
            loop {
                let t = self.bump();
                match t.kind {
                    Tok::Indent => level += 1,
                    Tok::Dedent => {
                        level -= 1;
                        if level == 0 {
                            break;
                        }
                    }
                    Tok::End => break,
                    _ => end = end.max(t.end),
                }
            }
        }
        let (kind, error) = match fail {
            Fail::Syntax => ("ERROR", true),
            Fail::TooDeep => (TOO_DEEP, false),
        };
        self.nodes.push(NodeData {
            kind,
            named: true,
            error,
            start,
            end: end.max(start),
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }

    fn statement_inner(&mut self) -> Result<Vec<usize>, Fail> {
        let t = self.tok();
        if t.kind == Tok::Error {
            return Err(Fail::Syntax);
        }
        if t.kind == Tok::Name {
            match self.text(t) {
                "class" => return self.class_definition().map(|id| vec![id]),
                "def" => return self.function_definition().map(|id| vec![id]),
                "async" => {
                    // `async async ...` would otherwise recurse once per token.
                    self.bump();
                    self.descend()?;
                    let result = self.statement_inner();
                    self.depth -= 1;
                    return result;
                }
                "if" => {
                    return self
                        .compound("if_statement", &["elif", "else"])
                        .map(|id| vec![id]);
                }
                "for" => return self.compound("for_statement", &["else"]).map(|id| vec![id]),
                "while" => {
                    return self
                        .compound("while_statement", &["else"])
                        .map(|id| vec![id]);
                }
                "with" => return self.compound("with_statement", &[]).map(|id| vec![id]),
                "try" => {
                    return self
                        .compound("try_statement", &["except", "else", "finally"])
                        .map(|id| vec![id]);
                }
                "match" if self.looks_like_match() => {
                    return self.compound("match_statement", &[]).map(|id| vec![id]);
                }
                _ => {}
            }
        }
        if self.at_op("@") {
            return self.decorated().map(|id| vec![id]);
        }
        self.simple_statements()
    }

    fn looks_like_match(&self) -> bool {
        // `match x:` at statement start; `match = 1` or `match(x)` stay expressions.
        let next = self.tokens.get(self.pos + 1);
        next.is_some_and(|n| {
            n.kind == Tok::Name
                || n.kind == Tok::Number
                || n.kind == Tok::Str
                || (n.kind == Tok::Op && self.text(*n) == "[")
        })
    }

    fn simple_statements(&mut self) -> Result<Vec<usize>, Fail> {
        let mut ids = vec![self.small_statement()?];
        while self.at_op(";") {
            self.bump();
            if matches!(self.tok().kind, Tok::Newline | Tok::End) {
                break;
            }
            ids.push(self.small_statement()?);
        }
        match self.tok().kind {
            Tok::Newline => {
                self.bump();
                Ok(ids)
            }
            Tok::End | Tok::Dedent => Ok(ids),
            _ => Err(Fail::Syntax),
        }
    }

    /// A keyword statement whose details the importer never inspects: keep its span.
    fn opaque(&mut self, kind: &'static str) -> Parsed {
        let start = self.bump().start;
        let mut end = self.prev_end();
        loop {
            let t = self.tok();
            if matches!(t.kind, Tok::Newline | Tok::End | Tok::Dedent | Tok::Indent)
                || (t.kind == Tok::Op && self.text(t) == ";")
            {
                break;
            }
            if t.kind == Tok::Error {
                return Err(Fail::Syntax);
            }
            end = t.end;
            self.bump();
        }
        Ok(self.node(kind, start, end, Vec::new()))
    }

    fn small_statement(&mut self) -> Parsed {
        let t = self.tok();
        if t.kind == Tok::Name {
            let kind = match self.text(t) {
                "pass" => Some("pass_statement"),
                "break" => Some("break_statement"),
                "continue" => Some("continue_statement"),
                "return" => Some("return_statement"),
                "import" => Some("import_statement"),
                "global" => Some("global_statement"),
                "nonlocal" => Some("nonlocal_statement"),
                "del" => Some("delete_statement"),
                "raise" => Some("raise_statement"),
                "assert" => Some("assert_statement"),
                "from" => {
                    let future = self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|n| self.text(*n) == "__future__");
                    Some(if future {
                        "future_import_statement"
                    } else {
                        "import_from_statement"
                    })
                }
                _ => None,
            };
            if let Some(kind) = kind {
                return self.opaque(kind);
            }
        }
        let start = self.tok().start;
        let left = self.expression_list()?;
        let inner = if self.at_op("=") {
            self.assignment(left)?
        } else if self.tok().kind == Tok::Op && AUGMENTED.contains(&self.text(self.tok())) {
            let op = self.bump();
            let op = self.anon(op);
            let right = self.expression_list()?;
            let (_, end) = self.span(right);
            self.node(
                "augmented_assignment",
                start,
                end,
                vec![
                    (Some("left"), left),
                    (Some("operator"), op),
                    (Some("right"), right),
                ],
            )
        } else if self.at_op(":") {
            // Annotated assignment `x: T = value`.
            self.bump();
            let annotation = self.expression()?;
            let mut children = vec![(Some("left"), left), (Some("type"), annotation)];
            let mut end = self.span(annotation).1;
            if self.at_op("=") {
                self.bump();
                let right = self.expression_list()?;
                end = self.span(right).1;
                children.push((Some("right"), right));
            }
            self.node("assignment", start, end, children)
        } else {
            left
        };
        let (_, end) = self.span(inner);
        Ok(self.node("expression_statement", start, end, vec![(None, inner)]))
    }

    fn assignment(&mut self, left: usize) -> Parsed {
        let (start, _) = self.span(left);
        self.expect_op("=")?;
        self.descend()?;
        let first = self.expression_list()?;
        let right = if self.at_op("=") {
            self.assignment(first)?
        } else {
            first
        };
        self.depth -= 1;
        let (_, end) = self.span(right);
        Ok(self.node(
            "assignment",
            start,
            end,
            vec![(Some("left"), left), (Some("right"), right)],
        ))
    }

    fn name(&mut self) -> Parsed {
        let t = self.tok();
        if t.kind != Tok::Name || is_reserved(self.text(t)) {
            return Err(Fail::Syntax);
        }
        self.bump();
        Ok(self.node("identifier", t.start, t.end, Vec::new()))
    }

    fn class_definition(&mut self) -> Parsed {
        let start = self.bump().start;
        let name = self.name()?;
        let mut children = vec![(Some("name"), name)];
        if self.at_op("(") {
            let arguments = self.argument_list()?;
            children.push((Some("superclasses"), arguments));
        }
        self.expect_op(":")?;
        let body = self.block()?;
        children.push((Some("body"), body));
        let (_, end) = self.span(body);
        Ok(self.node("class_definition", start, end, children))
    }

    fn function_definition(&mut self) -> Parsed {
        let start = self.bump().start;
        let name = self.name()?;
        let open = self.expect_op("(")?;
        let close_end = self.skip_balanced_to(")")?;
        let parameters = self.node("parameters", open.start, close_end, Vec::new());
        let mut children = vec![(Some("name"), name), (Some("parameters"), parameters)];
        if self.at_op("->") {
            self.bump();
            let returns = self.expression()?;
            children.push((Some("return_type"), returns));
        }
        self.expect_op(":")?;
        let body = self.block()?;
        children.push((Some("body"), body));
        let (_, end) = self.span(body);
        Ok(self.node("function_definition", start, end, children))
    }

    /// Consume tokens up to and including the bracket closing the one just opened.
    fn skip_balanced_to(&mut self, close: &str) -> Result<usize, Fail> {
        let mut level = 0usize;
        loop {
            let t = self.tok();
            match t.kind {
                Tok::End | Tok::Newline | Tok::Error => return Err(Fail::Syntax),
                Tok::Op => match self.text(t) {
                    "(" | "[" | "{" => level += 1,
                    ")" | "]" | "}" => {
                        if level == 0 {
                            if self.text(t) != close {
                                return Err(Fail::Syntax);
                            }
                            self.bump();
                            return Ok(t.end);
                        }
                        level -= 1;
                    }
                    _ => {}
                },
                _ => {}
            }
            self.bump();
        }
    }

    fn decorated(&mut self) -> Parsed {
        let start = self.tok().start;
        while self.at_op("@") {
            self.bump();
            self.expression()?;
            if self.tok().kind != Tok::Newline {
                return Err(Fail::Syntax);
            }
            self.bump();
        }
        let definition = if self.at_keyword("class") {
            self.class_definition()?
        } else if self.at_keyword("def") {
            self.function_definition()?
        } else if self.at_keyword("async") {
            self.bump();
            self.function_definition()?
        } else {
            return Err(Fail::Syntax);
        };
        let (_, end) = self.span(definition);
        Ok(self.node(
            "decorated_definition",
            start,
            end,
            vec![(Some("definition"), definition)],
        ))
    }

    /// A compound statement the importer reports but never interprets: its header
    /// is skipped up to `:`, and its blocks (plus continuation clauses) are parsed so
    /// syntax errors inside are still reported.
    fn compound(&mut self, kind: &'static str, clauses: &[&str]) -> Parsed {
        let start = self.tok().start;
        let mut children = Vec::new();
        let mut end;
        loop {
            self.bump();
            self.skip_header()?;
            let body = self.block()?;
            end = self.span(body).1;
            children.push((Some("body"), body));
            let t = self.tok();
            if t.kind == Tok::Name && clauses.contains(&self.text(t)) {
                continue;
            }
            break;
        }
        Ok(self.node(kind, start, end, children))
    }

    /// Skip a compound statement header up to its top-level `:`.
    fn skip_header(&mut self) -> Result<(), Fail> {
        let mut level = 0usize;
        loop {
            let t = self.tok();
            match t.kind {
                Tok::End | Tok::Newline | Tok::Error | Tok::Indent | Tok::Dedent => {
                    return Err(Fail::Syntax);
                }
                Tok::Op => match self.text(t) {
                    "(" | "[" | "{" => level += 1,
                    ")" | "]" | "}" => level = level.saturating_sub(1),
                    ":" if level == 0 => {
                        self.bump();
                        return Ok(());
                    }
                    _ => {}
                },
                _ => {}
            }
            self.bump();
        }
    }

    fn block(&mut self) -> Parsed {
        self.descend()?;
        let block = self.block_inner();
        self.depth -= 1;
        block
    }

    fn block_inner(&mut self) -> Parsed {
        let start = self.tok().start;
        if self.tok().kind != Tok::Newline {
            // `if x: pass` — simple statements on the header line.
            let ids = self.simple_statements()?;
            let end = self.prev_end();
            return Ok(self.node(
                "block",
                start,
                end,
                ids.into_iter().map(|id| (None, id)).collect(),
            ));
        }
        self.bump();
        if self.tok().kind != Tok::Indent {
            return Err(Fail::Syntax);
        }
        let start = self.bump().start;
        let mut children = Vec::new();
        loop {
            match self.tok().kind {
                Tok::Dedent => {
                    self.bump();
                    break;
                }
                Tok::End => break,
                Tok::Newline => {
                    self.bump();
                }
                Tok::Indent => {
                    let t = self.bump();
                    let id = self.recover(t.start, Fail::Syntax);
                    children.push((None, id));
                }
                _ => children.extend(self.statement().into_iter().map(|id| (None, id))),
            }
        }
        let end = children.last().map_or(start, |(_, id)| self.nodes[*id].end);
        Ok(self.node("block", start, end, children))
    }

    // --- expressions ------------------------------------------------------

    /// `a, b` without brackets becomes an `expression_list`.
    fn expression_list(&mut self) -> Parsed {
        let first = self.star_or_expression()?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let (start, mut end) = self.span(first);
        let mut children = vec![(None, first)];
        while self.at_op(",") {
            end = self.bump().end;
            if self.ends_expression_list() {
                break;
            }
            let item = self.star_or_expression()?;
            end = self.span(item).1;
            children.push((None, item));
        }
        Ok(self.node("expression_list", start, end, children))
    }

    fn ends_expression_list(&self) -> bool {
        let t = self.tok();
        matches!(t.kind, Tok::Newline | Tok::End | Tok::Dedent)
            || (t.kind == Tok::Op && matches!(self.text(t), "=" | ")" | "]" | "}" | ";" | ":"))
            || (t.kind == Tok::Op && AUGMENTED.contains(&self.text(t)))
    }

    fn star_or_expression(&mut self) -> Parsed {
        if self.at_op("*") {
            let t = self.bump();
            let value = self.expression()?;
            let (_, end) = self.span(value);
            return Ok(self.node("list_splat", t.start, end, vec![(None, value)]));
        }
        self.expression()
    }

    fn expression(&mut self) -> Parsed {
        if self.at_keyword("lambda") {
            return self.lambda();
        }
        let body = self.disjunction()?;
        if self.at_op(":=") {
            self.descend()?;
            self.bump();
            let value = self.expression()?;
            self.depth -= 1;
            let (start, _) = self.span(body);
            let (_, end) = self.span(value);
            return Ok(self.node(
                "named_expression",
                start,
                end,
                vec![(Some("name"), body), (Some("value"), value)],
            ));
        }
        if self.at_keyword("if") {
            // A conditional expression; inside a comprehension `if` is a filter,
            // which the caller handles, so only take it when `else` follows.
            let saved = (self.pos, self.depth);
            self.descend()?;
            self.bump();
            if let Ok(condition) = self.disjunction()
                && self.at_keyword("else")
            {
                self.bump();
                let alternative = self.expression()?;
                self.depth -= 1;
                let (start, _) = self.span(body);
                let (_, end) = self.span(alternative);
                return Ok(self.node(
                    "conditional_expression",
                    start,
                    end,
                    vec![(None, body), (None, condition), (None, alternative)],
                ));
            }
            (self.pos, self.depth) = saved;
        }
        Ok(body)
    }

    fn lambda(&mut self) -> Parsed {
        let start = self.bump().start;
        // Parameters up to the top-level `:`.
        while !self.at_op(":") {
            if matches!(self.tok().kind, Tok::Newline | Tok::End | Tok::Error) {
                return Err(Fail::Syntax);
            }
            self.bump();
        }
        self.bump();
        self.descend()?;
        let body = self.expression()?;
        self.depth -= 1;
        let (_, end) = self.span(body);
        Ok(self.node("lambda", start, end, vec![(Some("body"), body)]))
    }

    fn disjunction(&mut self) -> Parsed {
        let mut left = self.conjunction()?;
        while self.at_keyword("or") {
            let op = self.bump();
            let op = self.anon(op);
            let right = self.conjunction()?;
            left = self.binary("boolean_operator", left, op, right);
        }
        Ok(left)
    }

    fn conjunction(&mut self) -> Parsed {
        let mut left = self.inversion()?;
        while self.at_keyword("and") {
            let op = self.bump();
            let op = self.anon(op);
            let right = self.inversion()?;
            left = self.binary("boolean_operator", left, op, right);
        }
        Ok(left)
    }

    fn inversion(&mut self) -> Parsed {
        if self.at_keyword("not") {
            self.descend()?;
            let t = self.bump();
            let argument = self.inversion()?;
            self.depth -= 1;
            let (_, end) = self.span(argument);
            return Ok(self.node(
                "not_operator",
                t.start,
                end,
                vec![(Some("argument"), argument)],
            ));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Parsed {
        let first = self.binary_level(0)?;
        let mut children = vec![(None, first)];
        loop {
            let t = self.tok();
            let text = self.text(t);
            let is_cmp = (t.kind == Tok::Op && COMPARISONS.contains(&text))
                || (t.kind == Tok::Name && matches!(text, "in" | "is"))
                || (t.kind == Tok::Name
                    && text == "not"
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|n| self.text(*n) == "in"));
            if !is_cmp {
                break;
            }
            let op = self.bump();
            if (text == "not" || text == "is") && (self.at_keyword("in") || self.at_keyword("not"))
            {
                self.bump();
            }
            let op = self.anon(op);
            children.push((Some("operators"), op));
            let right = self.binary_level(0)?;
            children.push((None, right));
        }
        if children.len() == 1 {
            return Ok(first);
        }
        let (start, _) = self.span(first);
        let end = self.prev_end();
        Ok(self.node("comparison_operator", start, end, children))
    }

    /// Binary operators by precedence climbing: `|` `^` `&` shifts `+ -` then
    /// `* / // % @`.
    fn binary_level(&mut self, level: usize) -> Parsed {
        const LEVELS: [&[&str]; 6] = [
            &["|"],
            &["^"],
            &["&"],
            &["<<", ">>"],
            &["+", "-"],
            &["*", "/", "//", "%", "@"],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary_level(level + 1)?;
        while self.tok().kind == Tok::Op && LEVELS[level].contains(&self.text(self.tok())) {
            let op = self.bump();
            let op = self.anon(op);
            let right = self.binary_level(level + 1)?;
            left = self.binary("binary_operator", left, op, right);
        }
        Ok(left)
    }

    fn binary(&mut self, kind: &'static str, left: usize, op: usize, right: usize) -> usize {
        let (start, _) = self.span(left);
        let (_, end) = self.span(right);
        self.node(
            kind,
            start,
            end,
            vec![
                (Some("left"), left),
                (Some("operator"), op),
                (Some("right"), right),
            ],
        )
    }

    fn unary(&mut self) -> Parsed {
        if self.tok().kind == Tok::Op && matches!(self.text(self.tok()), "-" | "+" | "~") {
            self.descend()?;
            let t = self.bump();
            let op = self.anon(t);
            let argument = self.unary()?;
            self.depth -= 1;
            let (_, end) = self.span(argument);
            return Ok(self.node(
                "unary_operator",
                t.start,
                end,
                vec![(Some("operator"), op), (Some("argument"), argument)],
            ));
        }
        self.power()
    }

    fn power(&mut self) -> Parsed {
        let base = if self.at_keyword("await") {
            let t = self.bump();
            let value = self.primary()?;
            let (_, end) = self.span(value);
            self.node("await", t.start, end, vec![(None, value)])
        } else {
            self.primary()?
        };
        if self.at_op("**") {
            self.descend()?;
            let op = self.bump();
            let op = self.anon(op);
            let exponent = self.unary()?;
            self.depth -= 1;
            return Ok(self.binary("binary_operator", base, op, exponent));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Parsed {
        let mut value = self.atom()?;
        loop {
            if self.at_op(".") {
                self.bump();
                let attribute = self.name()?;
                let (start, _) = self.span(value);
                let (_, end) = self.span(attribute);
                value = self.node(
                    "attribute",
                    start,
                    end,
                    vec![(Some("object"), value), (Some("attribute"), attribute)],
                );
            } else if self.at_op("(") {
                let arguments = self.argument_list()?;
                let (start, _) = self.span(value);
                let (_, end) = self.span(arguments);
                value = self.node(
                    "call",
                    start,
                    end,
                    vec![(Some("function"), value), (Some("arguments"), arguments)],
                );
            } else if self.at_op("[") {
                self.descend()?;
                self.bump();
                let mut children = vec![(Some("value"), value)];
                while !self.at_op("]") {
                    let item = self.slice_item()?;
                    children.push((Some("subscript"), item));
                    if !self.at_op(",") {
                        break;
                    }
                    self.bump();
                }
                let close = self.expect_op("]")?;
                self.depth -= 1;
                let (start, _) = self.span(value);
                value = self.node("subscript", start, close.end, children);
            } else {
                return Ok(value);
            }
        }
    }

    fn slice_item(&mut self) -> Parsed {
        let start = self.tok().start;
        let mut parts = Vec::new();
        let mut is_slice = false;
        loop {
            if self.at_op(":") {
                is_slice = true;
                self.bump();
                continue;
            }
            if self.at_op(",") || self.at_op("]") {
                break;
            }
            parts.push((None, self.expression()?));
        }
        if !is_slice && parts.len() == 1 {
            return Ok(parts[0].1);
        }
        let end = self.prev_end();
        Ok(self.node("slice", start, end, parts))
    }

    fn argument_list(&mut self) -> Parsed {
        self.descend()?;
        let open = self.expect_op("(")?;
        let mut children = Vec::new();
        while !self.at_op(")") {
            let t = self.tok();
            let arg = if self.at_op("**") {
                self.bump();
                let value = self.expression()?;
                let (_, end) = self.span(value);
                self.node("dictionary_splat", t.start, end, vec![(None, value)])
            } else if self.at_op("*") {
                self.star_or_expression()?
            } else if t.kind == Tok::Name
                && self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|n| n.kind == Tok::Op && self.text(*n) == "=")
            {
                let name = self.name()?;
                self.bump();
                let value = self.expression()?;
                let (_, end) = self.span(value);
                self.node(
                    "keyword_argument",
                    t.start,
                    end,
                    vec![(Some("name"), name), (Some("value"), value)],
                )
            } else {
                let value = self.expression()?;
                if self.at_keyword("for") {
                    self.comprehension(value, "generator_expression")?
                } else {
                    value
                }
            };
            children.push((None, arg));
            if !self.at_op(",") {
                break;
            }
            self.bump();
        }
        let close = self.expect_op(")")?;
        self.depth -= 1;
        Ok(self.node("argument_list", open.start, close.end, children))
    }

    /// `body for x in xs if c ...` after `body`; consumes the clauses.
    fn comprehension(&mut self, body: usize, kind: &'static str) -> Parsed {
        let (start, _) = self.span(body);
        let mut children = vec![(Some("body"), body)];
        while self.at_keyword("for") || self.at_keyword("if") || self.at_keyword("async") {
            if self.at_keyword("async") {
                self.bump();
            }
            if self.at_keyword("for") {
                self.bump();
                let left = self.target_list()?;
                if !self.at_keyword("in") {
                    return Err(Fail::Syntax);
                }
                self.bump();
                let right = self.disjunction()?;
                children.push((None, left));
                children.push((None, right));
            } else {
                self.bump();
                let condition = self.disjunction()?;
                children.push((None, condition));
            }
        }
        let end = self.prev_end();
        Ok(self.node(kind, start, end, children))
    }

    fn target_list(&mut self) -> Parsed {
        let first = self.binary_level(0)?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let (start, _) = self.span(first);
        let mut children = vec![(None, first)];
        while self.at_op(",") {
            self.bump();
            if self.at_keyword("in") {
                break;
            }
            children.push((None, self.binary_level(0)?));
        }
        let end = self.prev_end();
        Ok(self.node("pattern_list", start, end, children))
    }

    #[allow(clippy::too_many_lines)] // One arm per atom kind.
    fn atom(&mut self) -> Parsed {
        let t = self.tok();
        match t.kind {
            Tok::Name => {
                let text = self.text(t);
                let kind = match text {
                    "True" => "true",
                    "False" => "false",
                    "None" => "none",
                    _ if is_reserved(text) => return Err(Fail::Syntax),
                    _ => "identifier",
                };
                self.bump();
                Ok(self.node(kind, t.start, t.end, Vec::new()))
            }
            Tok::Number => {
                self.bump();
                let text = self.text(t);
                let lower = text.to_ascii_lowercase();
                let float = !lower.starts_with("0x")
                    && (lower.contains('.') || lower.contains('e') || lower.ends_with('j'));
                Ok(self.node(
                    if float { "float" } else { "integer" },
                    t.start,
                    t.end,
                    Vec::new(),
                ))
            }
            Tok::Str => {
                self.bump();
                let first = self.node("string", t.start, t.end, Vec::new());
                if self.tok().kind != Tok::Str {
                    return Ok(first);
                }
                let mut children = vec![(None, first)];
                let mut end = t.end;
                while self.tok().kind == Tok::Str {
                    let next = self.bump();
                    end = next.end;
                    children.push((None, self.node("string", next.start, next.end, Vec::new())));
                }
                Ok(self.node("concatenated_string", t.start, end, children))
            }
            Tok::Op => match self.text(t) {
                "..." => {
                    self.bump();
                    Ok(self.node("ellipsis", t.start, t.end, Vec::new()))
                }
                "(" => self.parenthesized(),
                "[" => self.bracketed("[", "]", "list", "list_comprehension"),
                "{" => self.bracketed("{", "}", "set", "set_comprehension"),
                _ => Err(Fail::Syntax),
            },
            _ => Err(Fail::Syntax),
        }
    }

    fn parenthesized(&mut self) -> Parsed {
        self.descend()?;
        let open = self.bump();
        if self.at_op(")") {
            let close = self.bump();
            self.depth -= 1;
            return Ok(self.node("tuple", open.start, close.end, Vec::new()));
        }
        let first = if self.at_keyword("yield") {
            self.opaque_until_close()?
        } else {
            self.star_or_expression()?
        };
        if self.at_keyword("for") {
            let generator = self.comprehension(first, "generator_expression")?;
            let close = self.expect_op(")")?;
            self.depth -= 1;
            self.nodes[generator].start = open.start;
            self.nodes[generator].end = close.end;
            return Ok(generator);
        }
        if self.at_op(")") {
            let close = self.bump();
            self.depth -= 1;
            return Ok(self.node(
                "parenthesized_expression",
                open.start,
                close.end,
                vec![(None, first)],
            ));
        }
        let mut children = vec![(None, first)];
        while self.at_op(",") {
            self.bump();
            if self.at_op(")") {
                break;
            }
            children.push((None, self.star_or_expression()?));
        }
        let close = self.expect_op(")")?;
        self.depth -= 1;
        Ok(self.node("tuple", open.start, close.end, children))
    }

    fn opaque_until_close(&mut self) -> Parsed {
        let start = self.tok().start;
        let mut level = 0usize;
        loop {
            let t = self.tok();
            match t.kind {
                Tok::End | Tok::Newline | Tok::Error => return Err(Fail::Syntax),
                Tok::Op if matches!(self.text(t), "(" | "[" | "{") => level += 1,
                Tok::Op if matches!(self.text(t), ")" | "]" | "}") => {
                    if level == 0 {
                        break;
                    }
                    level -= 1;
                }
                _ => {}
            }
            self.bump();
        }
        let end = self.prev_end();
        Ok(self.node("yield", start, end, Vec::new()))
    }

    /// `[...]` lists and `{...}` sets/dicts, including comprehensions.
    fn bracketed(
        &mut self,
        open_op: &str,
        close_op: &str,
        kind: &'static str,
        comprehension: &'static str,
    ) -> Parsed {
        self.descend()?;
        let open = self.expect_op(open_op)?;
        let mut children = Vec::new();
        let mut kind = kind;
        let mut comprehension = comprehension;
        while !self.at_op(close_op) {
            let item = if self.at_op("**") {
                let t = self.bump();
                let value = self.expression()?;
                let (_, end) = self.span(value);
                kind = "dictionary";
                comprehension = "dictionary_comprehension";
                self.node("dictionary_splat", t.start, end, vec![(None, value)])
            } else {
                let key = self.star_or_expression()?;
                if open_op == "{" && self.at_op(":") {
                    self.bump();
                    let value = self.expression()?;
                    kind = "dictionary";
                    comprehension = "dictionary_comprehension";
                    let (start, _) = self.span(key);
                    let (_, end) = self.span(value);
                    self.node(
                        "pair",
                        start,
                        end,
                        vec![(Some("key"), key), (Some("value"), value)],
                    )
                } else {
                    key
                }
            };
            if self.at_keyword("for") && children.is_empty() {
                let generated = self.comprehension(item, comprehension)?;
                let close = self.expect_op(close_op)?;
                self.depth -= 1;
                self.nodes[generated].start = open.start;
                self.nodes[generated].end = close.end;
                return Ok(generated);
            }
            children.push((None, item));
            if !self.at_op(",") {
                break;
            }
            self.bump();
        }
        let close = self.expect_op(close_op)?;
        self.depth -= 1;
        if open_op == "{" && children.is_empty() {
            kind = "dictionary";
        }
        Ok(self.node(kind, open.start, close.end, children))
    }
}

fn is_reserved(word: &str) -> bool {
    matches!(
        word,
        "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

/// Parse a Python module. Never fails: syntax errors become `ERROR` nodes.
pub(crate) fn parse(source: &str) -> Tree {
    let tokens = Lexer {
        src: source,
        pos: 0,
        tokens: Vec::new(),
        indents: vec![0],
        brackets: Vec::new(),
        line_has_tokens: false,
    }
    .run();
    let mut parser = Parser {
        src: source,
        tokens,
        pos: 0,
        nodes: Vec::new(),
        depth: 0,
    };
    let root = parser.module();
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(i, _)| i + 1),
    );
    Tree {
        nodes: parser.nodes,
        line_starts,
        root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sexp(node: Node<'_>, source: &str) -> String {
        let mut cursor = node.walk();
        let children: Vec<String> = node
            .named_children(&mut cursor)
            .map(|child| sexp(child, source))
            .collect();
        if children.is_empty() {
            format!(
                "({} {:?})",
                node.kind(),
                node.utf8_text(source.as_bytes()).unwrap()
            )
        } else {
            format!("({} {})", node.kind(), children.join(" "))
        }
    }

    fn parse_sexp(source: &str) -> String {
        let tree = parse(source);
        sexp(tree.root_node(), source)
    }

    #[test]
    fn scene_structure_uses_tree_sitter_kinds_and_fields() {
        let source = "from manim import *\n\nclass S(Scene):\n    def construct(self):\n        c = Circle(radius=2, color=BLUE)  # comment\n        self.play(c.animate.shift(-UP * 2), run_time=1.5)\n";
        let tree = parse(source);
        let root = tree.root_node();
        let class = root.named_child(1).unwrap();
        assert_eq!(class.kind(), "class_definition");
        let name = class.child_by_field_name("name").unwrap();
        assert_eq!(name.utf8_text(source.as_bytes()).unwrap(), "S");
        let supers = class.child_by_field_name("superclasses").unwrap();
        assert_eq!(supers.kind(), "argument_list");
        let def = class
            .child_by_field_name("body")
            .unwrap()
            .named_child(0)
            .unwrap();
        assert_eq!(def.kind(), "function_definition");
        let body = def.child_by_field_name("body").unwrap();
        let mut cursor = body.walk();
        let statements: Vec<_> = body.named_children(&mut cursor).collect();
        assert_eq!(statements.len(), 2, "comments are not statements");
        let assignment = statements[0].named_child(0).unwrap();
        assert_eq!(assignment.kind(), "assignment");
        assert_eq!(assignment.start_position(), Point { row: 4, column: 8 });
        let call = statements[1].named_child(0).unwrap();
        assert_eq!(call.kind(), "call");
        assert_eq!(
            sexp(call.child_by_field_name("arguments").unwrap(), source),
            "(argument_list (call (attribute (attribute (identifier \"c\") (identifier \"animate\")) (identifier \"shift\")) (argument_list (binary_operator (unary_operator (identifier \"UP\")) (integer \"2\")))) (keyword_argument (identifier \"run_time\") (float \"1.5\")))"
        );
    }

    #[test]
    fn precedence_matches_python() {
        assert_eq!(
            parse_sexp("x = -a ** 2 + b * c\n"),
            "(module (expression_statement (assignment (identifier \"x\") (binary_operator (unary_operator (binary_operator (identifier \"a\") (integer \"2\"))) (binary_operator (identifier \"b\") (identifier \"c\"))))))"
        );
        assert_eq!(
            parse_sexp("t = (1, 2.0)\np = (3)\ns = r'a' \"b\"\n"),
            "(module (expression_statement (assignment (identifier \"t\") (tuple (integer \"1\") (float \"2.0\")))) (expression_statement (assignment (identifier \"p\") (parenthesized_expression (integer \"3\")))) (expression_statement (assignment (identifier \"s\") (concatenated_string (string \"r'a'\") (string \"\\\"b\\\"\")))))"
        );
    }

    #[test]
    fn compound_statements_and_continuations() {
        let source = "for i in range(3):\n    x = [i for i in y if i]\nif a:\n    pass\nelif b: pass\nelse:\n    y = {1: 2}\nz = (1 +\n     2)\nw = 1 + \\\n    2\n";
        let tree = parse(source);
        let root = tree.root_node();
        let mut cursor = root.walk();
        let kinds: Vec<_> = root.named_children(&mut cursor).map(|n| n.kind()).collect();
        assert_eq!(
            kinds,
            [
                "for_statement",
                "if_statement",
                "expression_statement",
                "expression_statement"
            ]
        );
        assert!(!has_error(root));
    }

    fn has_error(node: Node<'_>) -> bool {
        let mut cursor = node.walk();
        node.is_error() || node.children(&mut cursor).any(has_error)
    }

    #[test]
    fn syntax_errors_are_local_and_recoverable() {
        let source = "class S(Scene):\n    def construct(self):\n        self.play(Write(\n    def other(self):\n        pass\n";
        let tree = parse(source);
        assert!(has_error(tree.root_node()));
        let class = tree.root_node().named_child(0).unwrap();
        assert_eq!(class.kind(), "class_definition");
        let body = class.child_by_field_name("body").unwrap();
        let mut cursor = body.walk();
        let kinds: Vec<_> = body.named_children(&mut cursor).map(|n| n.kind()).collect();
        assert_eq!(kinds, ["function_definition", "function_definition"]);

        for broken in ["x = = 1\ny = 2\n", "x = 'open\n", "  x = 1\n", "def f(:\n"] {
            assert!(has_error(parse(broken).root_node()), "{broken:?}");
        }
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let source = format!("x = {}1\ny = 2\n", "-".repeat(100_000));
        let tree = parse(&source);
        let root = tree.root_node();
        assert_eq!(root.named_child(0).unwrap().kind(), TOO_DEEP);
        assert_eq!(root.named_child(1).unwrap().kind(), "expression_statement");
        let source = format!("x = {}1{}\n", "(".repeat(10_000), ")".repeat(10_000));
        assert_eq!(
            parse(&source).root_node().named_child(0).unwrap().kind(),
            TOO_DEEP
        );
        // Every recursive production is bounded, not just brackets and operators.
        for source in [
            format!("{}1\ny = 2\n", "a = ".repeat(100_000)),
            format!("x = {}1\ny = 2\n", "1 if c else ".repeat(100_000)),
            format!("x = {}1\ny = 2\n", "lambda: ".repeat(100_000)),
            format!("x = {}1\ny = 2\n", "a := ".repeat(100_000)),
            format!("{}x = 1\ny = 2\n", "async ".repeat(100_000)),
        ] {
            let tree = parse(&source);
            let root = tree.root_node();
            assert_eq!(root.named_child(0).unwrap().kind(), TOO_DEEP);
            assert_eq!(root.named_child(1).unwrap().kind(), "expression_statement");
        }
        // Deeply nested blocks become a too-deep statement inside the outer blocks.
        let mut source = String::new();
        for level in 0..3_000 {
            source.push_str(&" ".repeat(level));
            source.push_str("if x:\n");
        }
        source.push_str(&" ".repeat(3_000));
        source.push_str("pass\ny = 2\n");
        let tree = parse(&source);
        let mut pending = vec![tree.root_node()];
        let mut found = false;
        while let Some(node) = pending.pop() {
            found |= node.kind() == TOO_DEEP;
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        assert!(found);
        assert_eq!(
            tree.root_node().named_child(1).unwrap().kind(),
            "expression_statement"
        );
    }
}
