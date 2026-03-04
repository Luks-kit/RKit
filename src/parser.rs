use crate::lexer::{Token, TokenType};
use crate::ast::{Expr, Stmt, ExtendItem};
use crate::value::Value;

#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
pub enum Precedence {
    None,
    Assignment, // =
    Comparison, // == != < > <= >=
    Term,       // + -
    Factor,     // * /
    Call,       // . ()
    Primary,
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    pub fn parse(&mut self) -> Vec<Stmt> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.declaration());
        }
        statements
    }

    // --- Declarations & Statements ---

    fn declaration(&mut self) -> Stmt {
        match self.peek() {
            TokenType::Int 
            | TokenType::Str 
            | TokenType::Bool 
            | TokenType::Float 
            | TokenType::Ptr
            | TokenType::Byte => self.var_declaration(),
            TokenType::Import => self.import_decl(),
            TokenType::Fn => self.fn_declaration(),
            TokenType::Extern => self.extern_declaration(),
            TokenType::Struct => self.struct_declaration(),
            TokenType::Identifier(_) if self.is_var_declaration() => self.var_declaration(),
            TokenType::Extend => self.extend_declaration(),
            _ => self.statement(),
        }
    }
    
    fn import_decl(&mut self) -> Stmt {
       self.advance();
        let name = if let TokenType::Identifier(n) = self.consume_ident() { n }
            else { panic!("Expected module name after 'import'"); };
        self.consume(TokenType::Semicolon, "Expect ';' after import.");
        Stmt::Import { module_name: name }
    
    }

    fn var_declaration(&mut self) -> Stmt {
        let value_type = self.parse_type();
        let name = if 
            let TokenType::Identifier(n) = self.consume_ident() 
            { n } else { panic!("[line {}] Expect variable name", self.peek_line()); };

        self.consume(TokenType::Equal, "Expect '=' after variable name.");
        let initializer = self.expression();
        self.consume(TokenType::Semicolon, "Expect ';' after variable declaration.");

        Stmt::VarDecl { name, value_type, initializer }
    }

    fn fn_declaration(&mut self) -> Stmt {
        self.advance(); // consume 'fn'
        // Simplified: assuming 'fn type name(...)'
        let ret_type = self.parse_type();
        let name = if 
            let TokenType::Identifier(n) = self.consume_ident() { n } 
            else { panic!("[line {}] Expected name", self.peek_line()); };
        
        self.consume(TokenType::LParen, "Expect '(' after function name.");
        
        let mut params = Vec::new();
        if !self.check(&TokenType::RParen) {
            loop {
                // Parse type (int/str)
                let p_type = self.parse_type();
                
                // Parse name
                let p_name = if let TokenType::Identifier(n) = self.consume_ident() {
                    n
                } else {
                    panic!("Expected parameter name");
                };
                
                params.push((p_name, p_type));
                
                if !self.check(&TokenType::Comma) { break; }
                self.advance(); // consume ','
            }
        }

        self.consume(TokenType::RParen, "Expect ')' after params.");
        
        self.consume(TokenType::LBrace, "Expect '{' before body.");
        let body = self.block();
        
        Stmt::Function { name, params: params, return_type: ret_type, body }
    }
    
    fn extern_declaration(&mut self) -> Stmt {
        self.advance(); // consume 'extern'
        self.consume(TokenType::Fn, "Expect 'fn' after 'extern'.");
        
        let ret_type = self.parse_type(); // return type
        let name = if let TokenType::Identifier(n) = self.consume_ident() { n }
            else { panic!("Expected function name"); };
        
        self.consume(TokenType::LParen, "Expect '(' after function name.");
        
        let mut params = Vec::new();
        let mut variadic = false;
        if !self.check(&TokenType::RParen) {
            loop {
                if self.check(&TokenType::Variadic) {
                    // consume '...'
                    self.advance(); 
                    variadic = true;
                    break;
                }
                let p_type = format!("{:?}", self.advance());
                let p_name = if let TokenType::Identifier(n) = self.consume_ident() { n }
                    else { panic!("Expected parameter name"); };
                params.push((p_name, p_type));
                if !self.check(&TokenType::Comma) { break; }
                self.advance();
            }
        }

        self.consume(TokenType::RParen, "Expect ')' after params.");
        self.consume(TokenType::Semicolon, "Expect ';' after extern declaration.");
        
        Stmt::Extern { name, params, return_type: ret_type, variadic }
    }

    fn struct_declaration(&mut self) -> Stmt {
        self.advance(); // consume 'struct'
        let name = if let TokenType::Identifier(n) = self.consume_ident() { n }
            else { panic!("Expected struct name"); };
        self.consume(TokenType::LBrace, "Expect '{' after struct name.");
        
        let mut fields = Vec::new();
        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            let field_type = self.parse_type();
            let field_name = if let TokenType::Identifier(n) = self.consume_ident() { n }
            else { panic!("Expected field name"); };
            self.consume(TokenType::Semicolon, "Expect ';' after field.");
            fields.push((field_name, field_type));
        }
        self.consume(TokenType::RBrace, "Expect '}' after struct body.");
        Stmt::Struct { name, fields }
    }
    
    fn extend_declaration(&mut self) -> Stmt {
        self.advance(); // consume 'extend'
        let type_name = if let TokenType::Identifier(n) = self.consume_ident() { n }
            else { panic!("Expected type name after 'extend'"); };
        self.consume(TokenType::LBrace, "Expect '{' after type name.");

        let mut items = Vec::new();
        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            let item = match self.peek().clone() {
                TokenType::Init  => self.parse_init(),
                TokenType::Dinit => self.parse_dinit(),
                TokenType::Fn    => self.parse_method(),
                _ => panic!("[line {}] Expected init, dinit, or fn in extend block", self.peek_line()),
            };
            items.push(item);
        }
        self.consume(TokenType::RBrace, "Expect '}' after extend block.");
        Stmt::Extend { type_name, items }
    }

    fn parse_init(&mut self) -> ExtendItem {
        self.advance(); // consume 'init'
        self.consume(TokenType::LParen, "Expect '(' after 'init'.");
        let mut params = Vec::new();
        if !self.check(&TokenType::RParen) {
            loop {
                let p_type = self.parse_type();
                let p_name = if let TokenType::Identifier(n) = self.consume_ident() { n }
                    else { panic!("Expected parameter name"); };
                params.push((p_name, p_type));
                if !self.check(&TokenType::Comma) { break; }
                self.advance();
            }
        }
        self.consume(TokenType::RParen, "Expect ')' after init params.");
        self.consume(TokenType::LBrace, "Expect '{' before init body.");
        let body = self.block();
        ExtendItem::Init { params, body }
    }

    fn parse_dinit(&mut self) -> ExtendItem {
        self.advance(); // consume 'dinit'
        self.consume(TokenType::LBrace, "Expect '{' before dinit body.");
        let body = self.block();
        ExtendItem::Dinit { body }
    }

    fn parse_method(&mut self) -> ExtendItem {
        self.advance(); // consume 'fn'
        let return_type = self.parse_type();
        let name = if let TokenType::Identifier(n) = self.consume_ident() { n }
            else { panic!("Expected method name"); };
        self.consume(TokenType::LParen, "Expect '(' after method name.");
        let mut params = Vec::new();
        if !self.check(&TokenType::RParen) {
            loop {
                let p_type = self.parse_type();
                let p_name = if let TokenType::Identifier(n) = self.consume_ident() { n }
                    else { panic!("Expected parameter name"); };
                params.push((p_name, p_type));
                if !self.check(&TokenType::Comma) { break; }
                self.advance();
            }
        }
        self.consume(TokenType::RParen, "Expect ')' after method params.");
        self.consume(TokenType::LBrace, "Expect '{' before method body.");
        let body = self.block();
        ExtendItem::Method { name, params, return_type, body }
    }

    fn statement(&mut self) -> Stmt {
        match self.peek() {
            TokenType::If => self.if_statement(),
            TokenType::While => self.while_statement(),
            TokenType::LBrace => {
                self.advance();
                Stmt::Block(self.block())
            },
            TokenType::Return => {
                self.advance(); // consume 'return
                if self.check(&TokenType::Semicolon) {
                    self.advance();
                    return Stmt::Return(Expr::Literal(Value::Null));
                }
                let value = self.expression();
                self.consume(TokenType::Semicolon, "Expect ';' after return value.");
                Stmt::Return(value)
            }
            _ => {
                let expr = self.expression();
                self.consume(TokenType::Semicolon, "Expected ';' after expression.");
                Stmt::Expression(expr)
            }
        }
    }

    fn if_statement(&mut self) -> Stmt {
        self.advance(); // consume 'if'
        self.consume(TokenType::LParen, "Expect '(' after 'if'.");
        let condition = self.expression();
        self.consume(TokenType::RParen, "Expect ')' after if condition.");

        let then_branch = Box::new(self.statement());
        
        let mut else_branch = None;
        if self.check(&TokenType::Else) {
            self.advance();
            else_branch = Some(Box::new(self.statement()));
        }

        Stmt::If {
            condition,
            then_branch,
            else_branch,
        }
    }

    fn while_statement(&mut self) -> Stmt {
        self.advance(); // consume 'while'
        self.consume(TokenType::LParen, "Expect '(' after 'while'.");
        let condition = self.expression();
        self.consume(TokenType::RParen, "Expect ')' after condition.");
        
        let body = Box::new(self.statement());
        
        Stmt::While { condition, body }
    }

    fn block(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            stmts.push(self.declaration());
        }
        self.consume(TokenType::RBrace, "Expect '}' after block.");
        stmts
    }
    
    

    // --- Pratt Expression Parsing ---
        

    pub fn expression(&mut self) -> Expr {
        self.parse_precedence(Precedence::Assignment)
    }

    fn parse_precedence(&mut self, precedence: Precedence) -> Expr {
        let token = self.advance().clone();
        
        // Prefix rules
        let mut left = match token {
            TokenType::Literal(v) => Expr::Literal(v),
            TokenType::Identifier(n) => {
                if self.check(&TokenType::LBrace) 
                { self.struct_init(n) } 
                else { Expr::Variable(n) }
            }            
            TokenType::LParen => {
                let expr = self.expression();
                self.consume(TokenType::RParen, "Expect ')' after expression.");
                expr
            }, 
            TokenType::LBracket => {
                let mut elements = Vec::new();
                while !self.check(&TokenType::RBracket) && !self.is_at_end() {
                    elements.push(self.expression());
                    if !self.check(&TokenType::Comma) { break; }
                    self.advance();
                }
                self.consume(TokenType::RBracket, "Expect ']' after slice literal.");
                Expr::SliceLiteral(elements)
            }
            TokenType::Len => {
                self.consume(TokenType::LParen, "Expect '(' after 'len'.");
                let expr = self.expression();
                self.consume(TokenType::RParen, "Expect ')' after len argument.");
                Expr::Len(Box::new(expr))
            }
            TokenType::Cast => {
                self.consume(TokenType::LParen, "Expect '(' after 'cast'.");
                let target_type = self.parse_type();
                self.consume(TokenType::Comma, "Expect ',' after cast type.");
                let expr = self.expression();
                self.consume(TokenType::RParen, "Expect ')' after cast expression.");
                Expr::Cast { target_type, expr: Box::new(expr) }
            }
            TokenType::Amp => {
                // &strict x or &x
                if self.check(&TokenType::Strict) {
                    self.advance(); // consume 'strict'
                    let expr = self.parse_precedence(Precedence::Primary);
                    Expr::StrictRef(Box::new(expr))
                } else {
                    let expr = self.parse_precedence(Precedence::Primary);
                    Expr::Ref(Box::new(expr))
                }
            }
            TokenType::Minus => {
                let operand = self.parse_precedence(Precedence::Primary);
                Expr::Unary {
                    op: TokenType::Minus,
                    operand: Box::new(operand),
                }
            }
            TokenType::Not => {
                let operand = self.parse_precedence(Precedence::Primary);
                Expr::Unary { op: TokenType::Not, operand: Box::new(operand) }
            }


            _ => panic!("[Line {}] Unexpected token in expression: {:?}", self.peek_line(), token),
        };

        
        // Infix rules
        while precedence <= self.get_precedence(self.peek()) {
            let op_token = self.advance().clone();
            left = match op_token {
                TokenType::Plus | TokenType::Minus
                | TokenType::Star | TokenType::Slash 
                | TokenType::EqualEqual | TokenType::NotEqual
                | TokenType::Greater | TokenType::GreaterEqual
                | TokenType::Less | TokenType::LessEqual
                => self.binary(left, op_token),
                TokenType::Equal => self.assignment(left),
                TokenType::LParen => self.call(left),
                TokenType::Dot => {
                    let field = if let TokenType::Identifier(n) = self.consume_ident() { n }
                        else { panic!("Expected field name after '.'"); };
                    // method call or field access?
                    if self.check(&TokenType::LParen) {
                        self.advance(); // consume '('
                        let mut args = Vec::new();
                        if !self.check(&TokenType::RParen) {
                            loop {
                                args.push(self.expression());
                                if !self.check(&TokenType::Comma) { break; }
                                self.advance();
                            }
                        }
                        self.consume(TokenType::RParen, "Expect ')' after arguments.");
                        Expr::MethodCall {
                            object: Box::new(left),
                            method: field,
                            args,
                        }
                    } else {
                        Expr::FieldAccess { object: Box::new(left), field }
                    }
                }                
                TokenType::LBracket => {
                    let index = self.expression();
                    self.consume(TokenType::RBracket, "Expect ']' after index.");
                    Expr::Index { object: Box::new(left), index: Box::new(index) }
                }
                _ => left,
            };
        }

        left
    }

    fn struct_init(&mut self, name: String) -> Expr {
        self.advance(); // consume '{'
        let mut fields = Vec::new();
        while !self.check(&TokenType::RBrace) && !self.is_at_end() {
            // try named: ident ':'
            let (field_name, value) = if let TokenType::Identifier(n) = self.peek().clone() {
                self.advance();
                if self.check(&TokenType::Colon) {
                    self.advance(); // consume ':'
                    let val = self.expression();
                    (n, val)
                } else {
                    // positional — put the identifier back as an expression
                    (String::new(), Expr::Variable(n))
                }
            } else {
                (String::new(), self.expression())
            };
            fields.push((field_name, value));
            if !self.check(&TokenType::Comma) { break; }
            self.advance();
        }
        self.consume(TokenType::RBrace, "Expect '}' after struct init.");
        Expr::StructInit { name, fields }
    }

    fn binary(&mut self, left: Expr, op_tok: TokenType) -> Expr {
        let precedence = self.get_precedence(&op_tok);
        let right = self.parse_precedence(next_precedence(precedence));
        
        Expr::Binary {
            left: Box::new(left),
            op: op_tok,
            right: Box::new(right),
        }
    }
    
    fn assignment(&mut self, left: Expr) -> Expr {
        let value = self.parse_precedence(Precedence::Assignment);
        Expr::Assign {
            target: Box::new(left),
            value: Box::new(value),
        }
    }
    
    fn call(&mut self, callee: Expr) -> Expr {
        let mut args = Vec::new();
        if !self.check(&TokenType::RParen) {
            loop {
                args.push(self.expression());
                if !self.check(&TokenType::Comma) { break; }
                self.advance(); // consume ','
            }
        }
        self.consume(TokenType::RParen, "Expect ')' after arguments.");
        
        Expr::Call {
            callee: Box::new(callee),
            args,
        }
            

    }

    // --- Helpers ---

    fn get_precedence(&self, token: &TokenType) -> Precedence {
        match token {
            TokenType::EqualEqual | TokenType::NotEqual => Precedence::Comparison,
            TokenType::Less | TokenType::LessEqual => Precedence::Comparison,
            TokenType::Greater | TokenType::GreaterEqual => Precedence::Comparison,
            TokenType::Slash | TokenType::Star => Precedence::Factor, 
            TokenType::Plus | TokenType::Minus => Precedence::Term,
            TokenType::LParen => Precedence::Call,
            TokenType::Dot => Precedence::Call,
            TokenType::LBracket => Precedence::Call,
            TokenType::Equal => Precedence::Assignment,
            _ => Precedence::None,
        }
    }

    fn peek(&self) -> &TokenType {
        &self.tokens[self.current].kind
    }

    fn peek_line(&self) -> usize {
        self.tokens[self.current].line
    }

    fn advance(&mut self) -> &TokenType {
        if !self.is_at_end() { self.current += 1; }
        &self.tokens[self.current - 1].kind
    }

    fn check(&self, kind: &TokenType) -> bool {
        self.peek() == kind
    }

    fn consume(&mut self, kind: TokenType, msg: &str) {
        if self.check(&kind) { self.advance(); }
        else { 
            panic!("[line {}] {}", self.peek_line(), msg);
        }
    }

    fn consume_ident(&mut self) -> TokenType {
        let t = self.peek().clone();
        if let TokenType::Identifier(_) = t {
            self.advance();
            t
        } else {
            panic!("[line {}] Expected identifier, got {:?}", self.peek_line(), t);
        }
    }


    fn is_at_end(&self) -> bool {
        matches!(self.peek(), TokenType::EOF)
    }

   fn parse_type(&mut self) -> String {
        // [T] — dynamic slice
        if self.check(&TokenType::LBracket) {
            self.advance();
            let inner = self.parse_type();
            self.consume(TokenType::RBracket, "Expect ']' after slice type.");
            if self.check(&TokenType::Amp) {
                self.advance();
                return format!("[{}]&", inner);
            }
            return format!("[{}]", inner);
        }

        let base = match self.advance().clone() {
            TokenType::Identifier(n) => n,
            other => format!("{:?}", other),
        };

        // T[N] — fixed slice
        if self.check(&TokenType::LBracket) {
            self.advance();
            let size = if let TokenType::Literal(Value::Int(n)) = self.advance().clone() { n }
                else { panic!("Expected integer size in fixed slice type"); };
            self.consume(TokenType::RBracket, "Expect ']' after slice size.");
            let slice = format!("{}[{}]", base, size);
            if self.check(&TokenType::Amp) {
                self.advance();
                return format!("{}&", slice);
            }
            return slice;
        }

        // T* — heap owner
        if self.check(&TokenType::Star) {
            self.advance();
            return format!("{}*", base);
        }

        // T& or T strict&
        if self.check(&TokenType::Amp) {
            self.advance();
            return format!("{}&", base);
        }
        if self.check(&TokenType::Strict) {
            self.advance();
            self.consume(TokenType::Amp, "Expect '&' after 'strict'.");
            return format!("{} strict&", base);
        }

        base
    }    
    
    
    fn is_var_declaration(&self) -> bool {
        // look for: type [strict] [&] identifier
        let mut offset = 1;
        // skip 'strict' and '&' tokens
        loop {
            match self.tokens.get(self.current + offset).map(|t| &t.kind) {
                Some(TokenType::Strict) 
                | Some(TokenType::Amp) 
                | Some(TokenType::Star)
                | Some(TokenType::LBracket) 
                | Some(TokenType::RBracket) => offset += 1,
                Some(TokenType::Identifier(_)) => return true,
                _ => return false,
            }
        }
    }

}

fn next_precedence(p: Precedence) -> Precedence {
    // Helper to increment precedence for right-associativity if needed
    match p {
        Precedence::None => Precedence::Assignment,
        Precedence::Assignment => Precedence::Comparison,
        Precedence::Comparison => Precedence::Term,
        Precedence::Term => Precedence::Factor,
        Precedence::Factor => Precedence::Call,
        Precedence::Call => Precedence::Primary,
        Precedence::Primary => Precedence::Primary,
    }
}
