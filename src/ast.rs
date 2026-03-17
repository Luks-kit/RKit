use crate::lexer::{Span, TokenType};
use crate::value::Value;

#[derive(Debug, Clone)]
pub enum ExtendItem {
    Init {
        params: Vec<(String, String)>,
        body: Vec<Stmt>,
    },
    Dinit {
        body: Vec<Stmt>,
    },
    Method {
        name: String,
        params: Vec<(String, String)>,
        return_type: String,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub struct ToolMethod {
    pub name: String,
    pub params: Vec<(String, String)>,
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
        fields: Vec<(String, Expr)>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    SliceLiteral(Vec<Expr>),
    Len(Box<Expr>),
    Ref(Box<Expr>),
    StrictRef(Box<Expr>),
    Cast {
        target_type: String,
        expr: Box<Expr>,
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
        fields: Vec<(String, Expr)>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    SliceLiteral(Vec<Expr>),
    Len(Box<Expr>),
    Ref(Box<Expr>),
    StrictRef(Box<Expr>),
    Cast {
        target_type: String,
        expr: Box<Expr>,
    },
}

impl From<ExprKind> for Expr {
    fn from(value: ExprKind) -> Self {
        match value {
            ExprKind::Binary { left, op, right } => Expr::Binary { left, op, right },
            ExprKind::Unary { op, operand } => Expr::Unary { op, operand },
            ExprKind::Literal(v) => Expr::Literal(v),
            ExprKind::Variable(n) => Expr::Variable(n),
            ExprKind::Assign { target, value } => Expr::Assign { target, value },
            ExprKind::Call { callee, args } => Expr::Call { callee, args },
            ExprKind::FieldAccess { object, field } => Expr::FieldAccess { object, field },
            ExprKind::MethodCall {
                object,
                method,
                args,
            } => Expr::MethodCall {
                object,
                method,
                args,
            },
            ExprKind::StructInit { name, fields } => Expr::StructInit { name, fields },
            ExprKind::Index { object, index } => Expr::Index { object, index },
            ExprKind::SliceLiteral(items) => Expr::SliceLiteral(items),
            ExprKind::Len(expr) => Expr::Len(expr),
            ExprKind::Ref(expr) => Expr::Ref(expr),
            ExprKind::StrictRef(expr) => Expr::StrictRef(expr),
            ExprKind::Cast { target_type, expr } => Expr::Cast { target_type, expr },
        }
    }
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
        params: Vec<(String, String)>,
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
        params: Vec<(String, String)>,
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
    Import {
        module_name: String,
    },
    Function {
        name: String,
        params: Vec<(String, String)>,
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
        params: Vec<(String, String)>,
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

impl From<StmtKind> for Stmt {
    fn from(value: StmtKind) -> Self {
        match value {
            StmtKind::Expression(e) => Stmt::Expression(e),
            StmtKind::VarDecl {
                name,
                value_type,
                initializer,
            } => Stmt::VarDecl {
                name,
                value_type,
                initializer,
            },
            StmtKind::LetDecl { name, initializer } => Stmt::LetDecl { name, initializer },
            StmtKind::Block(stmts) => Stmt::Block(stmts),
            StmtKind::Import { module_name } => Stmt::Import { module_name },
            StmtKind::Function {
                name,
                params,
                return_type,
                body,
            } => Stmt::Function {
                name,
                params,
                return_type,
                body,
            },
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => Stmt::If {
                condition,
                then_branch,
                else_branch,
            },
            StmtKind::While { condition, body } => Stmt::While { condition, body },
            StmtKind::Return(e) => Stmt::Return(e),
            StmtKind::Extern {
                name,
                params,
                return_type,
                variadic,
            } => Stmt::Extern {
                name,
                params,
                return_type,
                variadic,
            },
            StmtKind::Struct { name, fields } => Stmt::Struct { name, fields },
            StmtKind::Extend { type_name, items } => Stmt::Extend { type_name, items },
            StmtKind::Tool { name, methods } => Stmt::Tool { name, methods },
            StmtKind::ExtendWith {
                type_name,
                tool_name,
                items,
            } => Stmt::ExtendWith {
                type_name,
                tool_name,
                items,
            },
        }
    }
}

impl Expr {
    pub fn new(kind: ExprKind, _span: Span) -> Self {
        kind.into()
    }
}

impl Stmt {
    pub fn new(kind: StmtKind, _span: Span) -> Self {
        kind.into()
    }
}
