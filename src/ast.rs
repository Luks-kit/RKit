use crate::value::Value;
use crate::lexer::TokenType;

#[derive(Debug, Clone)]
pub enum ExtendItem {
    Init {
        params: Vec<(String, String)>,  // (name, type) — no 'this'
        body: Vec<Stmt>,
    },
    Dinit {
        body: Vec<Stmt>,
    },
    Method {
        name: String,
        params: Vec<(String, String)>,  // first param is 'this: T&' or 'this: T strict&'
        return_type: String,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub enum Expr {
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
    Ref(Box<Expr>),        // &x
    StrictRef(Box<Expr>),  // &strict x
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Stmt {
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
    Struct{
        name: String,
        fields: Vec<(String, String)>,
    },
    Extend {
        type_name: String,
        items: Vec<ExtendItem>,
    },
}
