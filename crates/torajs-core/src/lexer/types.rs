//! Token / TemplatePart / Span / Spanned — public AST types
//! produced by `lexer::tokenize`. Pure data; `tokenize` itself
//! lives in `lexer.rs`.
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 20).

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    /// P8.1 — `#name` PrivateIdentifier (ES2022 §6.2.10). Holds the
    /// identifier body without the leading `#`. Distinct from `Ident`
    /// so the parser can route private-field declarations and
    /// `this.#x` accesses through a name-mangling step (encoding
    /// the class binding) without disturbing the public-name path.
    PrivateIdent(String),
    String(String),
    Number(f64),
    /// T-25 — `BigInt` literal. Holds the lexeme's digit body
    /// (without the trailing `n` and without `0x`/`0b`/`0o` radix
    /// prefix), plus the radix it was written in. The runtime parses
    /// the digits at allocation time. Always non-negative — leading
    /// `-` is tokenized as a unary op.
    BigInt {
        digits: String,
        radix: u32,
    },
    // keywords
    Let,
    /// P2.1 — `var` keyword. Distinct from `Let` so the parser can
    /// thread `is_var = true` into LetDecl, which the
    /// `desugar_var_hoist` pass uses to lift the declaration to the
    /// enclosing fn-body / top-level script (per ES spec §14.3.2.1).
    Var,
    Const,
    If,
    Else,
    True,
    False,
    While,
    For,
    Break,
    Continue,
    Function,
    Return,
    /// `type Foo = { x: number }` declares a structural type alias.
    Type,
    /// M4 — exception handling.
    Try,
    Catch,
    Finally,
    Throw,
    /// M5.1 — class / new / this. Single-class no-inheritance subset:
    /// `class C { f: T; constructor(...) {...} method(...): R {...} }`.
    /// Class is desugared post-parse into a TypeDecl + a set of FnDecls,
    /// so `class` / `this` / `new` exist only at the parser layer.
    Class,
    New,
    This,
    /// M5.2 — single inheritance: `class Sub extends Base { ... }`.
    /// `super(args)` is only valid inside a subclass constructor; it
    /// desugars to a call to the parent's `__cm_Parent__ctor`.
    Extends,
    Super,
    // punctuation
    Dot,
    Comma,
    Colon,
    Semi,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Star,
    StarStar,
    StarStarEq,
    Slash,
    Percent,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    ShlShl,
    ShrShr,
    /// `>>>` — unsigned right shift (logical, no sign-extension). JS
    /// `(x >>> 0)` is the canonical Number→UInt32 coercion idiom; the
    /// parser maps it to `BinOp::UShr` which the SSA layer lowers to
    /// LLVM's `lshr` instruction.
    ShrShrShr,
    /// `/pattern/flags` — regex literal. JS lexer disambiguates `/`
    /// between division and the start of a regex by inspecting the
    /// previous token: regex if the prev is missing / a punctuator /
    /// a recognized keyword (return, typeof, ...), division otherwise.
    /// The pattern + flags are kept as raw strings; the parser wraps
    /// the token in `Expr::Regex { pattern, flags }`. Actual matching
    /// machinery is a follow-up phase — for now, ssa_lower rejects
    /// regex emission with a "regex matching not yet implemented"
    /// message.
    Regex {
        pattern: String,
        flags: String,
    },
    Bang,
    /// `~` — bitwise not.
    Tilde,
    /// `...` — spread (in array literal) or rest (in destructuring,
    /// function param). Currently only the array-literal spread is
    /// lowered.
    DotDotDot,
    /// `null` — the JS / TS null sentinel. tr lowers it to a 0
    /// pointer for any pointer-shaped slot (Str / Obj / Arr / Closure
    /// / FnSig); primitive-shaped slots (number / boolean) can't be
    /// nullable in this subset (would need a tag bit).
    Null,
    /// `??` — nullish coalescing. Desugars to a ternary on the LHS's
    /// nullability.
    QuestionQuestion,
    /// `?.` — optional chaining for member access. `obj?.field`
    /// desugars to `obj == null ? null : obj.field`.
    QuestionDot,
    /// Template literal `\`hi ${name} bye\`` — produced as a single
    /// token carrying the alternating literal segments and the
    /// pre-tokenized interpolation expressions. Parser stitches them
    /// into a `+` chain at AST build time. Interpolations may use
    /// arbitrary expressions but must NOT contain `}` outside of
    /// balanced `{}` pairs (no inner template strings yet).
    Template {
        parts: Vec<TemplatePart>,
    },
    /// `?` — start of a ternary `cond ? a : b` expression.
    Question,
    Eq,
    EqEqEq,
    BangEqEq,
    /// V3-18 m3 — JS loose equality `==` / `!=`. Parser maps to
    /// `BinOp::LooseEq` / `BinOp::LooseNeq`. Spec §7.2.13.
    EqEq,
    BangEq,
    /// `+=`, `-=`, `*=`, `/=`, `%=` — compound assignment, parser
    /// desugars to the corresponding binop + ordinary assign.
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    /// `++`, `--` — increment / decrement; both pre + post forms.
    /// Parser desugars to `x = x + 1` / `x = x - 1`. The post-form's
    /// "yield-old-value" semantic is approximated as "yield-new" for
    /// now (the most common JS use is in for-loop step where it is
    /// equivalent).
    PlusPlus,
    MinusMinus,
    /// `do { ... } while (cond);` — parses to `Stmt::DoWhile`.
    Do,
    /// `switch (x) { case v: ... default: ... }` — parses to
    /// `Stmt::Switch`.
    Switch,
    Case,
    Default,
    /// `typeof x` — yields a string literal at runtime.
    TypeOf,
    /// `void x` — evaluates `x` (for side effects) then yields `undefined`.
    /// Per JS spec §13.5.2; the standard idiom for "produce undefined" in
    /// pre-ES2020 code (`void 0`).
    Void,
    /// `x instanceof C` — relational operator. tr is statically typed,
    /// so this is a compile-time decision based on the LHS's static
    /// type vs the named class (and its superclass chain).
    InstanceOf,
    /// Phase J — `yield e` produces the next value of a `function*`
    /// generator. Recognized only inside generator bodies; desugar
    /// rewrites the surrounding fn into a class with a `next()` state
    /// machine.
    Yield,
    /// Phase L — `async function f()` declares an async function whose
    /// body returns a Promise. desugar_async wraps the body's return
    /// value in a Promise and switches the surface return type from
    /// `T` to `Promise<T>`.
    Async,
    /// Phase L — `await <expr>` extracts the resolved value from a
    /// Promise. MVP desugar at parse time: `await e` ⇒ `e.value`
    /// (synchronous read, only well-defined for already-fulfilled
    /// promises in the current eager-fire model).
    Await,
    /// Phase K — `import { a, b } from "./x"` / `import x from "./x"` /
    /// `import * as ns from "./x"`. Single-file mode treats the import
    /// as a syntax-only declaration (no symbol resolution); K.2-K.4 will
    /// wire in cross-file linking.
    Import,
    /// Phase K — `export function/class/type/const/let X` modifier on
    /// a declaration, or `export { a, b }` re-export form. Single-file
    /// mode strips the modifier (no semantic effect).
    Export,
    FatArrow,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eof,
}

/// One slot inside a `Token::Template`. Either a raw literal segment
/// (the bytes between backticks / `${` / `}`) or a pre-tokenized
/// interpolation expression (everything inside `${…}`). The parser at
/// the Token::Template arm stitches them into `lit0 + expr0 + lit1 + …`.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    Lit(String),
    Expr(Vec<Spanned>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub token: Token,
    pub span: Span,
}
