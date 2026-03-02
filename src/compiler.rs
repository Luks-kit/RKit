use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::*;
use std::collections::HashMap;
use crate::lexer::TokenType;
use crate::ast::{Expr, Stmt};

pub struct Compiler<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub variables: HashMap<String, PointerValue<'ctx>>,
}

impl<'ctx> Compiler<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let vars = HashMap<String, PointerValue<'ctx>>;
        Compiler { context, module, builder, vars }
    }

    pub fn compile_expression(&self, expr: Expr) -> Result<IntValue<'ctx>, String> {
        match expr {
            Expr::Literal(val) => {
                match val {
                    // In RKit, everything is an i64 for now
                    crate::value::Value::Int(n) => {
                        Ok(self.context.i64_type().const_int(n as u64, true))
                    }
                    _ => Err("Unsupported literal type".into()),
                }
            }
            Expr::Binary { left, op, right } => {
                let lhs = self.compile_expression(*left)?;
                let rhs = self.compile_expression(*right)?;
                
                match op {
                    TokenType::Plus => self.builder.build_int_add(lhs, rhs, "addtmp").map_err(|e| e.to_string()),
                    TokenType::Minus => self.builder.build_int_sub(lhs, rhs, "subtmp").map_err(|e| e.to_string()),
                    TokenType::Star => self.builder.build_int_mul(lhs, rhs, "multmp").map_err(|e| e.to_string()),
                    TokenType::Slash => self.builder.build_int_signed_div(lhs, rhs, "divtmp").map_err(|e| e.to_string()),
                    _ => Err(format!("Unknown operator {:?}", op)),
                }
            }
            _ => Err("Expression type not implemented".into()),
        }
    }
}
