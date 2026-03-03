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
    borrow_state: HashMap<String, (usize, bool)>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            structs: HashMap::new(),
            current_return_type: None,
            errors: Vec::new(),
            borrow_state: HashMap::new(),
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

            Stmt::Function { name: _, params, return_type, body } => {
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
                 match &actual {
                    Some(LKitType::Ref(_)) | Some(LKitType::StrictRef(_)) => {
                        self.errors.push(TypeError::new(
                            "Cannot return a handle — it would outlive its referent"
                        ));
                    }
                    _ => {}
                }
                
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

            Expr::Assign { target, value } => {
                // 1. Determine the type of the assignment target
                let target_ty = match target.as_ref() {
                    Expr::Variable(name) => {
                        match self.lookup(name) {
                            Some(LKitType::StrictRef(inner)) => *inner.clone(),
                            Some(LKitType::Ref(_)) => {
                                self.errors.push(TypeError::new(
                                    format!("Cannot assign through shared handle '{}'", name)
                                ));
                                return None;
                            }
                            Some(t) => t.clone(),
                            None => {
                                self.errors.push(TypeError::new(
                                    format!("Undefined variable '{}'", name)
                                ));
                                return None;
                            }
                        }
                    }
                    Expr::FieldAccess { object, field } => {
                        let obj_ty = self.check_expr(object)?;
                        // unwrap handle, but reject shared handles
                        let base_ty = match obj_ty {
                            LKitType::StrictRef(inner) => *inner,
                            LKitType::Ref(_) => {
                                self.errors.push(TypeError::new(
                                    "Cannot assign to field through shared handle"
                                ));
                                return None;
                            }
                            other => other,
                        };
                        match base_ty {
                            LKitType::Struct(name) => {
                                match self.structs.get(&name) {
                                    Some(def) => match def.field_type(field) {
                                        Some(ty) => ty.clone(),
                                        None => {
                                            self.errors.push(TypeError::new(
                                                format!("No field '{}' on struct '{}'", field, name)
                                            ));
                                            return None;
                                        }
                                    },
                                    None => {
                                        self.errors.push(TypeError::new(
                                            format!("Unknown struct '{}'", name)
                                        ));
                                        return None;
                                    }
                                }
                            }
                            other => {
                                self.errors.push(TypeError::new(
                                    format!("Cannot assign to field on non-struct type {:?}", other)
                                ));
                                return None;
                            }
                        }
                    }
                    Expr::Index { object, index } => {
                        match self.check_expr(object)? {
                            LKitType::Slice(inner, _) |
                            LKitType::DynSlice(inner) => *inner,
                            other => {
                                self.errors.push(TypeError::new(
                                    format!("Cannot index-assign into non-slice type {:?}", other)
                                ));
                                return None;
                            }
                        }
                    }
                    _ => {
                        self.errors.push(TypeError::new("Invalid assignment target"));
                        return None;
                    }
                };

                // 2. Type-check the value being assigned and compare to target
                match self.check_expr(value) {
                    Some(val_ty) if val_ty != target_ty => {
                        self.errors.push(TypeError::new(format!(
                            "Cannot assign {:?} to {:?}", val_ty, target_ty
                        )));
                        None
                    }
                    other => other,
                }
            }
            
            Expr::Unary { op, operand } => {        
                match op { 
                TokenType::Not => match self.check_expr(operand)? {
                    LKitType::Bool => Some(LKitType::Bool),
                    other => {
                        self.errors.push(TypeError::new(format!("'!' on non-bool type {:?}", other)));
                        None
                    }
                },                
    
                TokenType::Minus => match self.check_expr(operand)? {
                        LKitType::Int   => Some(LKitType::Int),
                        LKitType::Float => Some(LKitType::Float),
                        other => {
                            self.errors.push(TypeError::new(
                                format!("Unary minus on non-numeric type {:?}", other)
                            ));
                            None
                        }
                    }
                    _ => { self.errors.push(TypeError::new(format!("Bad unary application: {:?} {:?}", op, operand))); None }
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
                let obj_ty = self.check_expr(object)?;
                // unwrap handle
                let base_ty = match obj_ty {
                    LKitType::Ref(inner) | LKitType::StrictRef(inner) => *inner,
                    other => other,
                };
                match base_ty {
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

            Expr::SliceLiteral(elements) => {
                if elements.is_empty() {
                    self.errors.push(TypeError::new("Cannot infer type of empty slice literal"));
                    return None;
                }
                let first = self.check_expr(&elements[0])?;
                for el in &elements[1..] {
                    match self.check_expr(el) {
                        Some(t) if t != first => {
                            self.errors.push(TypeError::new(format!(
                                "Slice literal has mixed types: {:?} and {:?}", first, t
                            )));
                            return None;
                        }
                        _ => {}
                    }
                }
                Some(LKitType::Slice(Box::new(first), elements.len() as u64))
            }

            Expr::Index { object, index } => {
                let idx_ty = self.check_expr(index)?;
                if idx_ty != LKitType::Int {
                    self.errors.push(TypeError::new("Slice index must be Int"));
                    return None;
                }

                if let Expr::Literal(Value::Int(n)) = index.as_ref() {
                    if let Some(LKitType::Slice(inner, size)) = self.check_expr(object) {
                        if *n < 0 || *n as u64 >= size {
                            self.errors.push(TypeError::new(format!(
                                "Index {} out of bounds for slice of size {}", n, size
                            )));
                            return None;
                        }
                        return Some(*inner);
                    }
                }

                match self.check_expr(object)? {
                    LKitType::Slice(inner, _) => Some(*inner),
                    LKitType::DynSlice(inner) => Some(*inner),
                    other => {
                        self.errors.push(TypeError::new(format!(
                            "Cannot index into non-slice type {:?}", other
                        )));
                        None
                    }
                }
            }

            Expr::Len(expr) => {
                match self.check_expr(expr)? {
                    LKitType::Slice(_, _) | LKitType::DynSlice(_) => Some(LKitType::Int),
                    other => {
                        self.errors.push(TypeError::new(format!(
                            "len() requires a slice, got {:?}", other
                        )));
                        None
                    }
                }
            }
            Expr::Ref(inner) => {
                match inner.as_ref() {
                    Expr::Variable(name) => {
                        let ty = self.lookup(name).ok_or_else(|| 
                                TypeError::new(format!("Undefined variable '{}'", name)))
                            .cloned();
                        match ty {
                            Ok(t) => {
                                if let Err(e) = self.borrow_shared(name) {
                                    self.errors.push(e);
                                    None
                                } else {
                                    Some(LKitType::Ref(Box::new(t)))
                                }
                            }
                            Err(e) => { self.errors.push(e); None }
                        }
                    }
                    _ => {
                        self.errors.push(TypeError::new("Can only take handle of a variable"));
                        None
                    }
                }
            }

            Expr::StrictRef(inner) => {
                match inner.as_ref() {
                    Expr::Variable(name) => {
                        let ty = self.lookup(name)
                            .ok_or_else(|| TypeError::new(format!("Undefined variable '{}'", name)))
                            .cloned();
                        match ty {
                            Ok(t) => {
                                if let Err(e) = self.borrow_exclusive(name) {
                                    self.errors.push(e);
                                    None
                                } else {
                                    Some(LKitType::StrictRef(Box::new(t)))
                                }
                            }
                            Err(e) => { self.errors.push(e); None }
                        }
                    }
                    _ => {
                        self.errors.push(TypeError::new("Can only take strict handle of a variable"));
                        None
                    }
                }
            }

            Expr::Deref(inner) => {
               Some(self.check_expr(inner)?)  
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

    // helper methods:
    fn borrow_shared(&mut self, name: &str) -> Result<(), TypeError> {
        let state = self.borrow_state.entry(name.to_string()).or_insert((0, false));
        if state.1 {
            Err(TypeError::new(format!(
                "Cannot create shared handle to '{}' — exclusive handle exists", name
            )))
        } else {
            state.0 += 1;
            Ok(())
        }
    }

    fn borrow_exclusive(&mut self, name: &str) -> Result<(), TypeError> {
        let state = self.borrow_state.entry(name.to_string()).or_insert((0, false));
        if state.1 {
            Err(TypeError::new(format!(
                "Cannot create exclusive handle to '{}' — exclusive handle already exists", name
            )))
        } else if state.0 > 0 {
            Err(TypeError::new(format!(
                "Cannot create exclusive handle to '{}' — shared handles exist", name
            )))
        } else {
            state.1 = true;
            Ok(())
        }
    }

}
