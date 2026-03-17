use crate::lexer::{Span, TokenType};
use crate::value::Value;

#[derive(Debug, Clone)]
pub enum ExtendItem {
    Init {
        params: Vec<(String, String)>, // (name, type) — no 'this'
        body: Vec<Stmt>,
    },
    Dinit {
        body: Vec<Stmt>,
    },
    Method {
        name: String,
        params: Vec<(String, String)>, // first param is 'this: T&' or 'this: T strict&'
        return_type: String,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub struct ToolMethod {
    pub name: String,
    pub params: Vec<(String, String)>, // (name, type) — no 'this', added implicitly
    pub return_type: String,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Binary {
        left: Box<Expr>,
        op: TokenType,
        right: Box<Expr>,
    },
    Unary {
        op: TokenType,
        operand: Box<Expr>,
    },
    Literal(Value),
    Variable(String),
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>, // (field name, value) — positional uses ""
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    SliceLiteral(Vec<Expr>),
    Len(Box<Expr>),
    Ref(Box<Expr>),       // &x
    StrictRef(Box<Expr>), // &strict x
    Cast {
        target_type: String,
        expr: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StmtKind {
    Expression(Expr),
    VarDecl {
        name: String,
        value_type: String,
        initializer: Expr,
    },
    LetDecl {
        name: String,
        initializer: Expr,
    },
    Block(Vec<Stmt>),
    Import {
        module_name: String,
    },
    Function {
        name: String,
        params: Vec<(String, String)>, // (name, type)
        return_type: String,
        body: Vec<Stmt>,
    },
    If {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        condition: Expr,
        body: Box<Stmt>,
    },
    Return(Expr),
    Extern {
        name: String,
        params: Vec<(String, String)>, // (name, type)
        return_type: String,
        variadic: bool,
    },
    Struct {
        name: String,
        fields: Vec<(String, String)>,
    },
    Extend {
        type_name: String,
        items: Vec<ExtendItem>,
    },
    Tool {
        name: String,
        methods: Vec<ToolMethod>,
    },
    ExtendWith {
        type_name: String,
        tool_name: String,
        items: Vec<ExtendItem>,
    },
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr { kind, span }
    }
}

impl Stmt {
    pub fn new(kind: StmtKind, span: Span) -> Self {
        Stmt { kind, span }
    }
}
