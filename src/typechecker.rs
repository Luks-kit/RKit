use std::collections::HashMap;
use crate::ast::{Expr, Stmt};
use crate::types::LKitType;
use crate::value::Value;
use crate::lexer::TokenType;
use crate::types::StructDef;

#[derive(Debug)]
pub struct TypeError {
    pub message: String,
}

impl TypeError {
    fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into() }
    }
}

pub struct TypeChecker {
    scopes: Vec<HashMap<String, LKitType>>,
    functions: HashMap<String, LKitType>,
    pub structs: HashMap<String, StructDef>,
    current_return_type: Option<LKitType>,
    pub errors: Vec<TypeError>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            structs: HashMap::new(),
            current_return_type: None,
            errors: Vec::new(),
        }
    }

    pub fn check(&mut self, stmts: &[Stmt]) {
        //Zero'th pass: register all structs        
        for stmt in stmts {
            if let Stmt::Struct { name, fields } = stmt {
                let typed_fields = fields.iter()
                    .filter_map(|(fname, ftype)| {
                        LKitType::from_str(ftype).map(|t| (fname.clone(), t))
                    })
                    .collect();
                self.structs.insert(name.clone(), StructDef {
                    name: name.clone(),
                    fields: typed_fields,
                });
            }
        }
        // First pass: register all function calls
        for stmt in stmts {
            if let Stmt::Function { name, params, return_type, .. } = stmt {
                let param_types = params.iter()
                    .filter_map(|(_, ty)| LKitType::from_str(ty))
                    .collect();
                let ret = LKitType::from_str(return_type)
                    .unwrap_or(LKitType::Void);
                self.functions.insert(name.clone(), LKitType::Function {
                    params: param_types,
                    ret: Box::new(ret),
                });
            }
            if let Stmt::Extern { name, params, return_type, .. } = stmt {
                let param_types = params.iter()
                    .filter_map(|(_, ty)| LKitType::from_str(ty))
                    .collect();
                let ret = LKitType::from_str(return_type)
                    .unwrap_or(LKitType::Void);
                self.functions.insert(name.clone(), LKitType::Function {
                    params: param_types,
                    ret: Box::new(ret),
                });
            }
        }

        // Second pass: check everything
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    fn push_scope(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop_scope(&mut self)  { self.scopes.pop(); }

    fn define(&mut self, name: &str, ty: LKitType) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), ty);
    }

    fn lookup(&self, name: &str) -> Option<&LKitType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl { name, value_type, initializer } => {
                let expected = match LKitType::from_str(value_type) {
                    Some(t) => t,
                    None => {
                        self.errors.push(TypeError::new(
                            format!("Unknown type '{}' for variable '{}'", value_type, name)
                        ));
                        return;
                    }
                };
                match self.check_expr(initializer) {
                    Some(actual) if actual != expected => {
                        self.errors.push(TypeError::new(format!(
                            "Type mismatch in '{}': expected {:?}, got {:?}",
                            name, expected, actual
                        )));
                    }
                    _ => {}
                }
                self.define(name, expected);
            }

            Stmt::LetDecl { name, initializer } => {
                match self.check_expr(initializer) {
                    Some(ty) => self.define(name, ty),
                    None => self.errors.push(TypeError::new(
                        format!("Cannot infer type for '{}'", name)
                    )),
                }
            }

            Stmt::Function { name, params, return_type, body } => {
                let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                self.current_return_type = Some(ret.clone());
                self.push_scope();
                for (param_name, param_type) in params {
                    if let Some(ty) = LKitType::from_str(param_type) {
                        self.define(param_name, ty);
                    } else {
                        self.errors.push(TypeError::new(
                            format!("Unknown type '{}' for param '{}'", param_type, param_name)
                        ));
                    }
                }
                for s in body { self.check_stmt(s); }
                self.pop_scope();
                self.current_return_type = None;
            }

            Stmt::Return(expr) => {
                let actual = self.check_expr(expr);
                match (&self.current_return_type, actual) {
                    (Some(expected), Some(actual)) if actual != *expected => {
                        self.errors.push(TypeError::new(format!(
                            "Return type mismatch: expected {:?}, got {:?}",
                            expected, actual
                        )));
                    }
                    _ => {}
                }
            }

            Stmt::If { condition, then_branch, else_branch } => {
                match self.check_expr(condition) {
                    Some(t) if t != LKitType::Bool => {
                        self.errors.push(TypeError::new(
                            format!("If condition must be Bool, got {:?}", t)
                        ));
                    }
                    _ => {}
                }
                self.push_scope();
                self.check_stmt(then_branch);
                self.pop_scope();
                if let Some(else_stmt) = else_branch {
                    self.push_scope();
                    self.check_stmt(else_stmt);
                    self.pop_scope();
                }
            }

            Stmt::While { condition, body } => {
                match self.check_expr(condition) {
                    Some(t) if t != LKitType::Bool => {
                        self.errors.push(TypeError::new(
                            format!("While condition must be Bool, got {:?}", t)
                        ));
                    }
                    _ => {}
                }
                self.push_scope();
                self.check_stmt(body);
                self.pop_scope();
            }

            Stmt::Block(stmts) => {
                self.push_scope();
                for s in stmts { self.check_stmt(s); }
                self.pop_scope();
            }

            Stmt::Expression(expr) => { self.check_expr(expr); }

            Stmt::Extern { .. } => {} // already registered in first pass
            Stmt::Struct { .. } => {} // already registered
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Option<LKitType> {
        match expr {
            Expr::Literal(val) => Some(match val {
                Value::Int(_)   => LKitType::Int,
                Value::Float(_) => LKitType::Float,
                Value::Bool(_)  => LKitType::Bool,
                Value::Str(_)   => LKitType::Str,
                Value::Null     => LKitType::Void,
                Value::Function(_) => return None,
            }),

            Expr::Variable(name) => {
                match self.lookup(name) {
                    Some(ty) => Some(ty.clone()),
                    None => {
                        self.errors.push(TypeError::new(
                            format!("Undefined variable '{}'", name)
                        ));
                        None
                    }
                }
            }

            Expr::Assign { name, value } => {
                let expected = match self.lookup(name) {
                    Some(ty) => ty.clone(),
                    None => {
                        self.errors.push(TypeError::new(
                            format!("Undefined variable '{}'", name)
                        ));
                        return None;
                    }
                };
                match self.check_expr(value) {
                    Some(actual) if actual != expected => {
                        self.errors.push(TypeError::new(format!(
                            "Cannot assign {:?} to variable '{}' of type {:?}",
                            actual, name, expected
                        )));
                        None
                    }
                    other => other,
                }
            }

            Expr::Binary { left, op, right } => {
                let l = self.check_expr(left)?;
                let r = self.check_expr(right)?;
                if l != r {
                    self.errors.push(TypeError::new(format!(
                        "Binary op {:?} on mismatched types {:?} and {:?}", op, l, r
                    )));
                    return None;
                }
                match op {
                    TokenType::EqualEqual | TokenType::NotEqual |
                    TokenType::Less | TokenType::LessEqual |
                    TokenType::Greater | TokenType::GreaterEqual
                        => Some(LKitType::Bool),
                    _   => Some(l),
                }
            }

            Expr::Call { callee, args } => {
                let name = match callee.as_ref() {
                    Expr::Variable(n) => n.clone(),
                    _ => {
                        self.errors.push(TypeError::new("Only direct calls supported"));
                        return None;
                    }
                };
                match self.functions.get(&name).cloned() {
                    Some(LKitType::Function { params, ret }) => {
                        if args.len() != params.len() {
                            // allow variadic mismatch for now
                        }
                        for (arg, expected) in args.iter().zip(params.iter()) {
                            match self.check_expr(arg) {
                                Some(actual) if &actual != expected => {
                                    self.errors.push(TypeError::new(format!(
                                        "Argument type mismatch in call to '{}': expected {:?}, got {:?}",
                                        name, expected, actual
                                    )));
                                }
                                _ => {}
                            }
                        }
                        Some(*ret)
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("Undefined function '{}'", name)
                        ));
                        None
                    }
                }
            }

            Expr::FieldAccess { object, field } => {
                match self.check_expr(object)? {
                    LKitType::Struct(name) => {
                        match self.structs.get(&name) {
                            Some(def) => match def.field_type(field) {
                                Some(ty) => Some(ty.clone()),
                                None => {
                                    self.errors.push(TypeError::new(
                                        format!("No field '{}' on struct '{}'", field, name)
                                    ));
                                    None
                                }
                            },
                            None => {
                                self.errors.push(TypeError::new(
                                    format!("Unknown struct '{}'", name)
                                ));
                                None
                            }
                        }
                    }
                    other => {
                        self.errors.push(TypeError::new(
                            format!("Cannot access field '{}' on non-struct type {:?}", field, other)
                        ));
                        None
                    }
                }
            }

            Expr::StructInit { name, fields } => {
                let def = match self.structs.get(name).cloned() {
                    Some(d) => d,
                    None => {
                        self.errors.push(TypeError::new(
                            format!("Unknown struct '{}'", name)
                        ));
                        return None;
                    }
                };
                // check each field value matches expected type
                for (i, (field_name, field_expr)) in fields.iter().enumerate() {
                    let expected = if field_name.is_empty() {
                        // positional
                        def.fields.get(i).map(|(_, t)| t.clone())
                    } else {
                        def.field_type(field_name).cloned()
                    };
                    match (expected, self.check_expr(field_expr)) {
                        (Some(exp), Some(actual)) if exp != actual => {
                            self.errors.push(TypeError::new(format!(
                                "Field '{}' of '{}': expected {:?}, got {:?}",
                                if field_name.is_empty() { format!("{}", i) } else { field_name.to_string() },
                                name, exp, actual
                            )));
                        }
                        _ => {}
                    }
                }
                Some(LKitType::Struct(name.clone()))
            }
        }
    }

    pub fn transform(&mut self, stmts: Vec<Stmt>) -> Vec<Stmt> {
        stmts.into_iter().map(|s| self.transform_stmt(s)).collect()
    }

    fn transform_stmt(&mut self, stmt: Stmt) -> Stmt {
        match stmt {
            Stmt::LetDecl { name, initializer } => {
                // Look up the inferred type from the scope
                let ty = self.lookup(&name)
                    .map(|t| t.to_str().to_string())
                    .unwrap_or_else(|| "Int".to_string());
                Stmt::VarDecl { name, value_type: ty, initializer }
            }

            Stmt::Function { name, params, return_type, body } => {
                self.push_scope();
                for (param_name, param_type) in &params {
                    if let Some(ty) = LKitType::from_str(param_type) {
                        self.define(param_name, ty);
                    }
                }
                let body = self.transform(body);
                self.pop_scope();
                Stmt::Function { name, params, return_type, body }
            }

            Stmt::Block(stmts) => {
                self.push_scope();
                let stmts = self.transform(stmts);
                self.pop_scope();
                Stmt::Block(stmts)
            }

            Stmt::If { condition, then_branch, else_branch } => {
                self.push_scope();
                let then_branch = Box::new(self.transform_stmt(*then_branch));
                self.pop_scope();
                let else_branch = else_branch.map(|e| {
                    self.push_scope();
                    let e = Box::new(self.transform_stmt(*e));
                    self.pop_scope();
                    e
                });
                Stmt::If { condition, then_branch, else_branch }
            }

            Stmt::While { condition, body } => {
                self.push_scope();
                let body = Box::new(self.transform_stmt(*body));
                self.pop_scope();
                Stmt::While { condition, body }
            }

            // Everything else passes through unchanged
            other => other,
        }
    }
}
