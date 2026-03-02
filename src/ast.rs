use crate::value::Value;
use crate::lexer::TokenType;

#[derive(Debug, Clone)]
pub enum Expr {
    Binary {
        left: Box<Expr>,
        op: TokenType,
        right: Box<Expr>,
    },
    Literal(Value),
    Variable(String),
    Assign {
        name: String,
        value: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expression(Expr),
    VarDecl {
        name: String,
        value_type: String,
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
}
