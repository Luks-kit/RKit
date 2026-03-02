mod lexer;
mod ast;
mod value;
mod env;
mod parser;
mod interpreter;
mod compiler;

use inkwell::context::Context;
use inkwell::values::FunctionValue;
use crate::compiler::Compiler;
use crate::lexer::TokenType;
use crate::ast::Expr;


fn main() {
    let context = Context::create();
    let compiler = Compiler::new(&context, "rkit_module");

    // 1. Manually create the 'main' function
    let i64_type = context.i64_type();
    let fn_type = i64_type.fn_type(&[], false);
    let main_fn = compiler.module.add_function("main", fn_type, None);

    // 2. Create entry block and position builder
    let entry = context.append_basic_block(main_fn, "entry");
    compiler.builder.position_at_end(entry);

    // 3. Compile an expression: 10 + 20
    let test_expr = Expr::Binary {
        left: Box::new(Expr::Literal(crate::value::Value::Int(10))),
        op: TokenType::Plus,
        right: Box::new(Expr::Literal(crate::value::Value::Int(20))),
    };

    let result = compiler.compile_expression(test_expr).unwrap();
    let built = compiler.builder.build_return(Some(&result));
    // 4. Dump the IR so you can see it!
    compiler.module.print_to_stderr();


}
