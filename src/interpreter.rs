use crate::ast::{Expr, Stmt};
use crate::value::{Value, FunctionData};
use crate::env::Environment;
use crate::lexer::TokenType;
use std::sync::Arc;

pub struct Interpreter {
    pub env: Environment,
}

pub enum ControlFlow {
    None,
    Return(Value),
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
        }
    }

    pub fn interpret(&mut self, statements: Vec<Stmt>) {
        for stmt in statements {
            if let Err(e) = self.execute(stmt) {
                eprintln!("Runtime Error: {}", e);
                break;
            }
        }
    }

    fn execute(&mut self, stmt: Stmt) -> Result<ControlFlow, String> {
        match stmt {
            Stmt::If { condition, then_branch, else_branch } => {
                if is_truthy(self.evaluate(condition)?) {
                    self.execute(*then_branch)?;
                } else if let Some(else_stmt) = else_branch {
                    self.execute(*else_stmt)?;
                }
                Ok(ControlFlow::None)
            }
            
            Stmt::While { condition, body } => {
                // We use a native Rust while loop to power our language's while loop
                while is_truthy(self.evaluate(condition.clone())?) {
                    self.execute(*body.clone())?;
                }
                Ok(ControlFlow::None)
            }

            Stmt::Expression(expr) => {
                let value = self.evaluate(expr)?;
                // Only print if it's not Null to keep it clean
                if value != Value::Null {
                    println!("{}", value.to_string());
                }
                Ok(ControlFlow::None)
            }
            Stmt::VarDecl { name, initializer, .. } => {
                let value = self.evaluate(initializer)?;
                self.env.define(name, value);
                Ok(ControlFlow::None)
            } 

            Stmt::Block(stmts) => {
                self.env.push();
                for s in stmts {
                    let flow = self.execute(s)?;
                    if let ControlFlow::Return(_) = flow {
                        self.env.pop(); // Don't forget to pop!
                        return Ok(flow);
                    }
                }
                self.env.pop();
                Ok(ControlFlow::None)
            }            
            Stmt::Function { name, params, return_type: _, body} 
            => {
                let param_names = params.into_iter().map(|(name, _type)| name).collect();
            
                let function_value = Value::Function(Arc::new(FunctionData {
                    params: param_names,
                    body,
                }));

                // Define the function name in the environment
                self.env.define(name, function_value);           
                Ok(ControlFlow::None)
            }
            Stmt::Return(e) => {
                let val = self.evaluate(e)?;
                Ok(ControlFlow::Return(val))
            }
            _ => Err("Statement type not yet implemented".to_string()),
        }
    }
    
    
    fn evaluate(&mut self, expr: Expr) -> Result<Value, String> {
        match expr {
            Expr::Literal(val) => Ok(val),
            Expr::Variable(name) => self.env.get(&name)
                .ok_or_else(|| format!("Undefined variable '{}'", name)),
            
            Expr::Binary { left, op, right } => {
                let l = self.evaluate(*left)?;
                let r = self.evaluate(*right)?;
                self.apply_binary(l, op, r)
            }
            Expr::Assign { name, value } => {
                let val = self.evaluate(*value)?;
                self.env.assign(name, val.clone())?;
                Ok(val) // Assignment expressions usually return the assigned value
            }
            Expr::Call { callee, args } => {
                // 1. Evaluate the callee (it should result in a Value::Function)
                let function_val = self.evaluate(*callee)?;
                
                if let Value::Function(func_data) = function_val {
                    // 2. Evaluate all arguments before entering the function scope
                    let mut evaluated_args = Vec::new();
                    for arg in args {
                        evaluated_args.push(self.evaluate(arg)?);
                    }

                    // 3. Safety check: do the argument counts match?
                    if evaluated_args.len() != func_data.params.len() {
                        return Err(format!("Expected {} arguments but got {}.", 
                            func_data.params.len(), evaluated_args.len()));
                    }

                    // 4. Create a new scope for the function execution
                    self.env.push();

                    // 5. Bind arguments to parameter names in the new scope
                    for (name, value) in func_data.params.iter().zip(evaluated_args) {
                        self.env.define(name.clone(), value);
                    }

                    // 6. Execute the body
                    let mut return_value = Value::Null;
                    for stmt in &func_data.body {
                        let flow = self.execute(stmt.clone())?;
                        if let ControlFlow::Return(val) = flow {
                            return_value = val;
                            break; // Stop executing further statements
                        }
                    }

                    // 7. Pop the scope back to where we were before the call
                    self.env.pop();

                    Ok(return_value)
                } else {
                    Err("Can only call functions.".to_string())
                }
            }
            _ => Err("Expression type not yet implemented".to_string()),
        }
    }

    fn apply_binary(&self, left: Value, op_tok: TokenType, right: Value) -> Result<Value, String> {
    match (left, op_tok, right) {
        // Arithmetic
        (Value::Int(a), TokenType::Plus, Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::Int(a), TokenType::Minus, Value::Int(b)) => Ok(Value::Int(a - b)),
        (Value::Int(a), TokenType::Star, Value::Int(b)) => Ok(Value::Int(a * b)),
        (Value::Int(a), TokenType::Slash, Value::Int(b)) => Ok(Value::Int(a / b)),
        // Comparisons
        (Value::Int(a), TokenType::EqualEqual, Value::Int(b)) => Ok(Value::Bool(a == b)),
        (Value::Int(a), TokenType::NotEqual,   Value::Int(b)) => Ok(Value::Bool(a != b)),
        (Value::Int(a), TokenType::GreaterEqual,Value::Int(b)) => Ok(Value::Bool(a >= b)),
        (Value::Int(a), TokenType::LessEqual, Value::Int(b)) => Ok(Value::Bool(a <= b)),
        (Value::Int(a), TokenType::Less, Value::Int(b)) => Ok(Value::Bool(a < b)),
        (Value::Int(a), TokenType::Greater, Value::Int(b)) => Ok(Value::Bool(a > b)),
         
        // String Equality
        (Value::Str(a), TokenType::EqualEqual, Value::Str(b)) => Ok(Value::Bool(a == b)),

        (l, o, r) => Err(format!("Invalid operation {:?} on {:?} and {:?}", o, l, r)),
    }
}}

fn is_truthy(value: Value) -> bool {
        match value {
            Value::Null => false,
            Value::Bool(b) => b,
            Value::Int(i) => i != 0, // 0 is false, everything else is true
            _ => true,
        }
    }

