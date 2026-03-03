use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::types::BasicType;
use inkwell::values::*;
use inkwell::types::BasicTypeEnum;
use inkwell::types::BasicMetadataTypeEnum;
use inkwell::{IntPredicate, FloatPredicate};
use inkwell::AddressSpace;
use std::collections::HashMap;
use crate::value::Value;
use crate::lexer::TokenType;
use crate::ast::{Expr, Stmt};
use crate::types::StructDef;
use crate::types::LKitType;


pub struct VarSlot<'ctx> {
    pub ptr: PointerValue<'ctx>,
    pub ty: BasicTypeEnum<'ctx>,
    pub is_ref: bool,
    pub type_name: String,
}

pub struct Compiler<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub struct_defs: HashMap<String, StructDef>,
    pub variables: HashMap<String, VarSlot<'ctx>>,

}

impl<'ctx> Compiler<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let variables: HashMap<String, VarSlot<'ctx>> = HashMap::new();
        let struct_defs: HashMap<String, StructDef> = HashMap::new();
        Compiler { context, module, builder, struct_defs, variables }
    } 

    pub fn compile_expression(&self, expr: Expr) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            Expr::Literal(val) => match val {
                Value::Int(i)   => Ok(self.context.i64_type().const_int(i as u64, true).as_basic_value_enum()),
                Value::Float(f) => Ok(self.context.f64_type().const_float(f).as_basic_value_enum()),
                Value::Bool(b)  => Ok(self.context.bool_type().const_int(b as u64, false).as_basic_value_enum()),
                Value::Str(s)   => {
                    let global = self.builder.build_global_string_ptr(&s, "str")
                        .map_err(|e| e.to_string())?;
                    Ok(global.as_pointer_value().as_basic_value_enum())
                },
                Value::Null        => Ok(self.context.i64_type().const_int(0, false).as_basic_value_enum()),
                Value::Function(_) => Err("Cannot use function as literal value".into()),
            },

            Expr::Variable(name) => {
                let slot = self.variables.get(&name)
                    .ok_or_else(|| format!("Undefined variable: {}", name))?;
                let val = self.builder.build_load(slot.ty, slot.ptr, &name)
                    .map_err(|e| e.to_string())?;
                if slot.is_ref {
                    // strip handle wrapper to get pointee type
                    let pointee_type_name = slot.type_name
                        .trim_end_matches('&')
                        .trim_end_matches("strict")
                        .trim()
                        .to_string();
                    let pointee_ty = self.type_str_to_llvm(&pointee_type_name);
                    self.builder.build_load(pointee_ty, val.into_pointer_value(), &name)
                        .map_err(|e| e.to_string())
                } else {
                    Ok(val)
                }
            }

           Expr::Assign { target, value } => {
                let val = self.compile_expression(*value)?;
                let (ptr, _) = self.compile_lvalue(&target)?;
                self.builder.build_store(ptr, val)
                    .map_err(|e| e.to_string())?;
                Ok(val)
            }, 
            
           Expr::Unary { op, operand } => {
                let val = self.compile_expression(*operand)?;
                match op {
                    TokenType::Minus => match val {
                        BasicValueEnum::IntValue(v) =>
                            Ok(self.builder.build_int_neg(v, "negtmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                        BasicValueEnum::FloatValue(v) =>
                            Ok(self.builder.build_float_neg(v, "fnegtmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                        _ => Err("Unary minus on non-numeric value".into()),
                    },
                    TokenType::Not => match val {
                        BasicValueEnum::IntValue(v) =>
                            Ok(self.builder.build_not(v, "nottmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                        _ => Err("'!' on non-bool value".into()),
                    },
                    _ => Err(format!("Unknown unary operator {:?}", op)),
                }
            }

           Expr::Binary { left, op, right } => {
                let lhs = self.compile_expression(*left)?;
                let rhs = self.compile_expression(*right)?;
                match (lhs, rhs) {
                    (BasicValueEnum::IntValue(l), BasicValueEnum::IntValue(r)) => {
                        let result = match op {
                            // Arithmetic: Wrap the result in BasicValueEnum to match the comparison arms
                            TokenType::Plus => self.builder.build_int_add(l, r, "addtmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Minus => self.builder.build_int_sub(l, r, "subtmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Star => self.builder.build_int_mul(l, r, "multmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Slash => self.builder.build_int_signed_div(l, r, "divtmp")
                                .map(|v| v.as_basic_value_enum()),

                            // Comparisons: These already return BasicValueEnum because of your .map()
                            TokenType::EqualEqual => self.builder.build_int_compare(IntPredicate::EQ, l, r, "eqtmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::NotEqual => self.builder.build_int_compare(IntPredicate::NE, l, r, "netmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Less => self.builder.build_int_compare(IntPredicate::SLT, l, r, "lttmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::LessEqual => self.builder.build_int_compare(IntPredicate::SLE, l, r, "letmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Greater => self.builder.build_int_compare(IntPredicate::SGT, l, r, "gttmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::GreaterEqual => self.builder.build_int_compare(IntPredicate::SGE, l, r, "getmp")
                                .map(|v| v.as_basic_value_enum()),

                            _ => return Err(format!("Unknown operator {:?}", op)),
                    };

                // Now 'result' is consistently Result<BasicValueEnum, BuilderError>
                result.map_err(|e| e.to_string()) 
                    },
                    (BasicValueEnum::FloatValue(l), BasicValueEnum::FloatValue(r)) => {
                        let result = match op {
                            TokenType::Plus  => self.builder.build_float_add(l, r, "faddtmp"),
                            TokenType::Minus => self.builder.build_float_sub(l, r, "fsubtmp"),
                            TokenType::Star  => self.builder.build_float_mul(l, r, "fmultmp"),
                            TokenType::Slash => self.builder.build_float_div(l, r, "fdivtmp"),
                            TokenType::EqualEqual => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OEQ,l,r,"feqtmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            TokenType::NotEqual  => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::ONE,l,r,"fnetmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                            TokenType::Less       => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OLT,l,r,"flttmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            TokenType::LessEqual  => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OLE,l,r,"fletmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                            TokenType::Greater    => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OGT,l,r,"fgttmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            TokenType::GreaterEqual => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OGE,l,r,"fgetmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            _ => return Err(format!("Unknown operator {:?}", op)),
                        };
                        result.map(|v| v.as_basic_value_enum()).map_err(|e| e.to_string())
                    },
                    _ => Err("Type mismatch in binary expression".into()),
                }
            },

            Expr::Call { callee, args } => {
                let callee_name = match *callee {
                    Expr::Variable(name) => name,
                    _ => return Err("Only direct function calls supported".into()),
                };
                let function = self.module.get_function(&callee_name)
                    .ok_or_else(|| format!("Undefined function: {}", callee_name))?;
                let compiled_args: Vec<BasicMetadataValueEnum> = args.into_iter()
                    .map(|a| self.compile_expression(a).map(|v| v.into()))
                    .collect::<Result<_, _>>()?;
                let call = self.builder.build_call(function, &compiled_args, "calltmp")
                    .map_err(|e| e.to_string())?;
                match call.try_as_basic_value() {
                    ValueKind::Basic(val) => Ok(val),
                    ValueKind::Instruction(_) => Ok(self.context.i64_type().const_int(0, false).as_basic_value_enum()),
                }   
            },

            Expr::StructInit { name, fields } => {
                let struct_ty = self.get_struct_type(&name)?;
                let alloca = self.builder.build_alloca(struct_ty, &name)
                    .map_err(|e| e.to_string())?;
                let def = self.struct_defs.get(&name).cloned()
                    .ok_or_else(|| format!("Unknown struct '{}'", name))?;
                for (i, (_, val_expr)) in fields.iter().enumerate() {
                    let val = self.compile_expression(val_expr.clone())?;
                    let ptr = self.builder.build_struct_gep(struct_ty, alloca, i as u32, "field")
                        .map_err(|e| e.to_string())?;
                    self.builder.build_store(ptr, val)
                        .map_err(|e| e.to_string())?;
                }
                Ok(self.builder.build_load(struct_ty, alloca, &name)
                    .map_err(|e| e.to_string())?)
            }
            
            Expr::FieldAccess { object, field } => {
                let vname = match *object {
                    Expr::Variable(ref vname) => vname.clone(),
                    _ => return Err("Complex field access not yet supported".into()),
                };
                let slot = self.variables.get(&vname)
                    .ok_or_else(|| format!("Undefined variable '{}'", vname))?;

                // resolve struct name from type_name, stripping any handle wrapper
                let struct_name = slot.type_name
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                    .trim()
                    .to_string();

                // if it's a handle, load the pointer first
                let struct_ptr = if slot.is_ref {
                    self.builder.build_load(
                        self.context.ptr_type(AddressSpace::default()),
                        slot.ptr,
                        &vname
                    ).map_err(|e| e.to_string())?.into_pointer_value()
                } else {
                    slot.ptr
                };

                let def = self.struct_defs.get(&struct_name).cloned()
                    .ok_or_else(|| format!("Unknown struct '{}'", struct_name))?;
                let idx = def.field_index(&field)
                    .ok_or_else(|| format!("No field '{}' on '{}'", field, struct_name))? as u32;
                let struct_ty = self.get_struct_type(&struct_name)?;
                let field_ty = self.type_str_to_llvm(&def.fields[idx as usize].1.to_str());

                let field_ptr = self.builder.build_struct_gep(struct_ty, struct_ptr, idx, &field)
                    .map_err(|e| e.to_string())?;
                self.builder.build_load(field_ty, field_ptr, &field)
                    .map_err(|e| e.to_string())
            },

            Expr::SliceLiteral(elements) => {
                if elements.is_empty() {
                    return Err("Cannot compile empty slice literal".into());
                }
                let compiled: Vec<BasicValueEnum> = elements.into_iter()
                    .map(|e| self.compile_expression(e))
                    .collect::<Result<_, _>>()?;
                let elem_ty = compiled[0].get_type();
                let n = compiled.len() as u32;
                let array_ty = match elem_ty {
                    BasicTypeEnum::IntType(t)   => t.array_type(n),
                    BasicTypeEnum::FloatType(t) => t.array_type(n),
                    BasicTypeEnum::StructType(t) => t.array_type(n),
                    BasicTypeEnum::PointerType(t) => t.array_type(n),
                    BasicTypeEnum::ArrayType(t) => t.array_type(n),
                    BasicTypeEnum::VectorType(t) => t.array_type(n),
                    _ => return Err("Unsupported element type in slice literal".into()),
                }; 
                let alloca = self.builder.build_alloca(array_ty, "slice")
                    .map_err(|e| e.to_string())?;
                for (i, val) in compiled.iter().enumerate() {
                    let ptr = unsafe {
                        self.builder.build_gep(array_ty, alloca, &[
                            self.context.i64_type().const_int(0, false),
                            self.context.i64_type().const_int(i as u64, false),
                        ], "elem_ptr").map_err(|e| e.to_string())?
                    };
                    self.builder.build_store(ptr, *val)
                        .map_err(|e| e.to_string())?;
                }
                self.builder.build_load(array_ty, alloca, "slice_val")
                    .map_err(|e| e.to_string())
            }

            Expr::Index { object, index } => {
                let name = match *object {
                    Expr::Variable(ref n) => n.clone(),
                    _ => return Err("Complex index expressions not yet supported".into()),
                };
                let slot = self.variables.get(&name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                let is_const_index = matches!(*index, Expr::Literal(Value::Int(_)));
                let idx_val = self.compile_expression(*index)?.into_int_value(); 

                match slot.ty {
                    BasicTypeEnum::ArrayType(arr_ty) => {
                        if !is_const_index {
                            let len = arr_ty.len();
                            // bounds check
                            let len_val = self.context.i64_type().const_int(len as u64, false);
                            let in_bounds = self.builder.build_int_compare(
                                IntPredicate::ULT, idx_val, len_val, "bounds_check"
                            ).map_err(|e| e.to_string())?;
                            let func = self.builder.get_insert_block()
                                .unwrap().get_parent().unwrap();
                            let ok_bb    = self.context.append_basic_block(func, "inbounds");
                            let fail_bb  = self.context.append_basic_block(func, "outofbounds");
                            self.builder.build_conditional_branch(in_bounds, ok_bb, fail_bb)
                                .map_err(|e| e.to_string())?;

                            // out of bounds — call abort
                            self.builder.position_at_end(fail_bb);
                            let abort = self.module.get_function("abort").unwrap_or_else(|| {
                                let ty = self.context.void_type().fn_type(&[], false);
                                self.module.add_function("abort", ty, Some(inkwell::module::Linkage::External))
                            });
                            self.builder.build_call(abort, &[], "")
                                .map_err(|e| e.to_string())?;
                            self.builder.build_unreachable()
                                .map_err(|e| e.to_string())?;

                            self.builder.position_at_end(ok_bb);
                        }
                        let elem_ty = arr_ty.get_element_type();
                        let ptr = unsafe {
                            self.builder.build_gep(slot.ty, slot.ptr, &[
                                self.context.i64_type().const_int(0, false),
                                idx_val,
                            ], "elem_ptr").map_err(|e| e.to_string())?
                        };
                        self.builder.build_load(elem_ty, ptr, "elem")
                            .map_err(|e| e.to_string())
                    }
                    _ => Err(format!("Cannot index into non-array variable '{}'", name)),
                }
            }

            Expr::Len(expr) => {
                let name = match *expr {
                    Expr::Variable(n) => n,
                    _ => return Err("len() argument must be a variable".into()),
                };
                let slot = self.variables.get(&name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                match slot.ty {
                    BasicTypeEnum::ArrayType(arr_ty) => {
                        Ok(self.context.i64_type()
                            .const_int(arr_ty.len() as u64, false)
                            .as_basic_value_enum())
                    }
                    _ => Err(format!("len() requires a slice, '{}' is not an array", name)),
                }
            }
            Expr::Ref(inner) | Expr::StrictRef(inner) => {
                match *inner {
                    Expr::Variable(name) => {
                        let slot = self.variables.get(&name)
                            .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                        Ok(slot.ptr.as_basic_value_enum())
                    }
                    _ => Err("Can only take handle of a variable".into()),
                }
            }

            Expr::Deref(inner) => {
                self.compile_expression(*inner)
            }
        }
    }
    
    pub fn compile_statement(&mut self, stmt: Stmt) -> Result<(), String> {
        match stmt {
            Stmt::VarDecl { name, value_type, initializer } => {
                let initial_val = self.compile_expression(initializer)?;
                let is_ref = value_type.ends_with('&');
                let ty = if is_ref {
                    self.context.ptr_type(AddressSpace::default()).into()
                } else {
                    initial_val.get_type()
                };
                let alloca = self.builder.build_alloca(ty, &name)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(alloca, initial_val)
                    .map_err(|e| e.to_string())?;
                self.variables.insert(name, VarSlot { ptr: alloca, ty, is_ref, type_name: value_type.clone() });
                Ok(())
            }

            Stmt::Expression(expr) => {
                self.compile_expression(expr)?;
                Ok(())
            }
            
            Stmt::Return(Expr::Literal(Value::Null)) => {
                self.builder.build_return(None)
                    .map_err(|e| e.to_string())?;
                Ok(())
            }

            Stmt::Return(expr) => {
                let val = self.compile_expression(expr)?;
                self.builder.build_return(Some(&val))
                    .map_err(|e| e.to_string())?;
                Ok(())
            }

            Stmt::Block(stmts) => {
                for s in stmts {
                    self.compile_statement(s)?;
                }
                Ok(())
            }

            Stmt::Function { name, params, return_type, body } => {
                // Build parameter types
                let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                    .map(|(_, ty)| self.type_str_to_llvm(ty).into())
                    .collect();

                let fn_type = match return_type.as_str() {
                    "Int" => self.context.i64_type().fn_type(&param_types, false),
                    "Float" => self.context.f64_type().fn_type(&param_types, false),
                    "Bool" => self.context.bool_type().fn_type(&param_types, false),
                    "Str" => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, false),
                    _ => self.context.i64_type().fn_type(&param_types, false), // default
                };

                let function = self.module.add_function(&name, fn_type, None);
                let entry = self.context.append_basic_block(function, "entry");
                self.builder.position_at_end(entry);

                // Save outer scope, create new one
                let outer_vars = std::mem::take(&mut self.variables);

                // Bind parameters to allocas
                for (i, (param_name, param_type)) in params.iter().enumerate() {
                    let is_ref = param_type.ends_with('&');
                    let ty = if is_ref {
                        self.context.ptr_type(AddressSpace::default()).into()
                    } else {
                       self.type_str_to_llvm(param_type)
                    };
                    let alloca = self.builder.build_alloca(ty, param_name)
                        .map_err(|e| e.to_string())?;
                    let param_val = function.get_nth_param(i as u32)
                        .ok_or_else(|| format!("Missing param {}", i))?;
                    self.builder.build_store(alloca, param_val)
                        .map_err(|e| e.to_string())?;
                    self.variables.insert(param_name.clone(), VarSlot { ptr: alloca, ty, is_ref, type_name: param_type.clone() });
                }

                // Compile body
                for s in body {
                    self.compile_statement(s)?;
                }

                // Restore outer scope
                self.variables = outer_vars;
                Ok(())
            }

            Stmt::If { condition, then_branch, else_branch } => {
                let cond_val = self.compile_expression(condition)?
                    .into_int_value();

                let function = self.builder.get_insert_block()
                    .unwrap().get_parent().unwrap();

                let then_bb = self.context.append_basic_block(function, "then");
                let else_bb = self.context.append_basic_block(function, "else");
                let merge_bb = self.context.append_basic_block(function, "merge");

                self.builder.build_conditional_branch(cond_val, then_bb, else_bb)
                    .map_err(|e| e.to_string())?;

                // Then branch
                self.builder.position_at_end(then_bb);
                self.compile_statement(*then_branch)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(merge_bb)
                        .map_err(|e| e.to_string())?;
                }

                // Else branch
                self.builder.position_at_end(else_bb);
                if let Some(else_stmt) = else_branch {
                    self.compile_statement(*else_stmt)?;
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(merge_bb)
                        .map_err(|e| e.to_string())?;
                }

                self.builder.position_at_end(merge_bb);
                Ok(())
            }

            Stmt::While { condition, body } => {
                let function = self.builder.get_insert_block()
                    .unwrap().get_parent().unwrap();

                let cond_bb  = self.context.append_basic_block(function, "while_cond");
                let body_bb  = self.context.append_basic_block(function, "while_body");
                let after_bb = self.context.append_basic_block(function, "while_after");

                self.builder.build_unconditional_branch(cond_bb)
                    .map_err(|e| e.to_string())?;

                // Condition
                self.builder.position_at_end(cond_bb);
                let cond_val = self.compile_expression(condition)?
                    .into_int_value();
                self.builder.build_conditional_branch(cond_val, body_bb, after_bb)
                    .map_err(|e| e.to_string())?;

                // Body
                self.builder.position_at_end(body_bb);
                self.compile_statement(*body)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb)
                        .map_err(|e| e.to_string())?;
                }

                self.builder.position_at_end(after_bb);
                Ok(())
            }
            Stmt::Extern { name, params, return_type, variadic } => {
                let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                    .map(|(_, ty)| self.type_str_to_llvm(ty).into()).collect(); 
                let fn_type = match return_type.as_str() {
                    "Int"   => self.context.i64_type().fn_type(&param_types, variadic),
                    "Float" => self.context.f64_type().fn_type(&param_types, variadic),
                    "Bool"  => self.context.bool_type().fn_type(&param_types, variadic),
                    "Str"   => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, variadic),
                    "Void"  => self.context.void_type().fn_type(&param_types, variadic),
                    _       => self.context.i64_type().fn_type(&param_types, variadic),
                };
                
                self.module.add_function(&name, fn_type, Some(inkwell::module::Linkage::External));
                Ok(())
            }
            Stmt::Struct { name, fields } => {
                let typed_fields: Vec<(String, LKitType)> = fields.iter()
                    .filter_map(|(n, t)| LKitType::from_str(t).map(|ty| (n.clone(), ty)))
                    .collect();
                self.struct_defs.insert(name.clone(), StructDef { name, fields: typed_fields });
                Ok(())
            }

            Stmt::LetDecl { .. } => unreachable!("LetDecl should be folded by type checker"),
        }
    }
    

    fn value_to_llvm_type(&self, val: &Value) -> BasicTypeEnum<'ctx> {
        match val {
            Value::Int(_)  => self.context.i64_type().into(),
            Value::Float(_) => self.context.f64_type().into(),
            Value::Bool(_) => self.context.bool_type().into(),
            Value::Str(_)  => self.context.ptr_type(AddressSpace::default()).into(),
            Value::Null    => self.context.i64_type().into(), // or a unit type
            Value::Function(_) => self.context.ptr_type(AddressSpace::default()).into(),
        }
    }
    fn type_str_to_llvm(&self, ty: &str) -> BasicTypeEnum<'ctx> {
        match ty {
            "Int" => self.context.i64_type().into(),
            "Float" => self.context.f64_type().into(),
            "Bool" => self.context.bool_type().into(),
            "Str" => self.context.ptr_type(AddressSpace::default()).into(),
            other if other.ends_with('&') => {
                self.context.ptr_type(AddressSpace::default()).into()
            }  
            other if other.starts_with('[') => {
                // [T] — dynamic slice: { ptr, i64 len, i64 cap }
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.ptr_type(AddressSpace::default());
                self.context.struct_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false).into()
            }
            other if other.contains('[') => {
                // T[N] — fixed slice
                if let Some(bracket) = other.find('[') {
                    let base = &other[..bracket];
                    let n: u32 = other[bracket+1..other.len()-1].parse().unwrap_or(0);
                    let elem_ty = self.type_str_to_llvm(base);
                    return elem_ty.array_type(n).into();
                }
                self.context.i64_type().into()
            }

            other   => {
                // try to resolve as a struct
                if let Some(def) = self.struct_defs.get(other) {
                    let field_types: Vec<BasicTypeEnum> = def.fields.iter()
                        .map(|(_, t)| self.type_str_to_llvm(&t.to_str()))
                        .collect();
                    self.context.struct_type(&field_types, false).into()
                } else {
                    // fallback — should never hit if type checker did its job
                    eprintln!("WARNING: unknown type '{}', defaulting to i64", other);
                    self.context.i64_type().into()
                }
            }
         }
    }
    
    fn compile_lvalue(&self, expr: &Expr) -> Result<(PointerValue<'ctx>, BasicTypeEnum<'ctx>), String> {
        match expr {
            Expr::Variable(name) => {
                let slot = self.variables.get(name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                if slot.is_ref {
                    // deref the handle to get the pointer it points to
                    let handle_val = self.builder.build_load(slot.ty, slot.ptr, name)
                        .map_err(|e| e.to_string())?;
                    Ok((handle_val.into_pointer_value(), slot.ty))
                } else {
                    Ok((slot.ptr, slot.ty))
                }
            }
          
            Expr::FieldAccess { object, field } => {
                let var_name = match object.as_ref() {
                    Expr::Variable(n) => n.clone(),
                    _ => return Err("Complex field assignment not yet supported".into()),
                };
                let slot = self.variables.get(&var_name)
                    .ok_or_else(|| format!("Undefined variable '{}'", var_name))?;

                // deref handle if needed
                let struct_ptr = if slot.is_ref {
                    let handle_val = self.builder.build_load(
                        self.context.ptr_type(AddressSpace::default()),
                        slot.ptr,
                        &var_name
                    ).map_err(|e| e.to_string())?;
                    handle_val.into_pointer_value()
                } else {
                    slot.ptr
                };

                // strip handle wrapper from type name: "Point strict&" -> "Point"
                let struct_name = slot.type_name
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                    .trim()
                    .to_string();

                let def = self.struct_defs.get(&struct_name).cloned()
                    .ok_or_else(|| format!("Unknown struct '{}'", struct_name))?;
                let idx = def.field_index(field)
                    .ok_or_else(|| format!("No field '{}' on '{}'", field, struct_name))? as u32;
                let struct_ty = self.get_struct_type(&struct_name)?;
                let field_ty = self.type_str_to_llvm(&def.fields[idx as usize].1.to_str());
                let ptr = self.builder.build_struct_gep(struct_ty, struct_ptr, idx, field)
                    .map_err(|e| e.to_string())?;
                Ok((ptr, field_ty))
            }

            Expr::Index { object, index } => {
                let name = match object.as_ref() {
                    Expr::Variable(n) => n.clone(),
                    _ => return Err("Complex index assignment not yet supported".into()),
                };
                let slot = self.variables.get(&name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                let idx_val = self.compile_expression(*index.clone())?.into_int_value();
                match slot.ty {
                    BasicTypeEnum::ArrayType(arr_ty) => {
                        let elem_ty = arr_ty.get_element_type();
                        let ptr = unsafe {
                            self.builder.build_gep(slot.ty, slot.ptr, &[
                                self.context.i64_type().const_int(0, false),
                                idx_val,
                            ], "elem_ptr").map_err(|e| e.to_string())?
                        };
                        Ok((ptr, elem_ty))
                    }
                    _ => Err(format!("Cannot index-assign into non-array '{}'", name)),
                }
            }
            _ => Err("Invalid lvalue".into()),
        }
    }


    // Helper to get LLVM struct type:
    fn get_struct_type(&self, name: &str) -> Result<inkwell::types::StructType<'ctx>, String> {
        let def = self.struct_defs.get(name)
            .ok_or_else(|| format!("Unknown struct '{}'", name))?;
        let field_types: Vec<BasicTypeEnum> = def.fields.iter()
            .map(|(_, ty)| self.type_str_to_llvm(&ty.to_str()))
            .collect();
        Ok(self.context.struct_type(&field_types, false))
    }

}
