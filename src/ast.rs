use crate::value::Value;
use crate::lexer::TokenType;

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
    Deref(Box<Expr>),      // explicit deref if needed later
}

#[derive(Debug, Clone)]
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
}
