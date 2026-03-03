use crate::lexer::{Token, TokenType};
use crate::ast::{Expr, Stmt};

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
            TokenType::Int | TokenType::Str 
            | TokenType::Bool | TokenType::Float => self.var_declaration(),
            TokenType::Fn => self.fn_declaration(),
            TokenType::Extern => self.extern_declaration(),
            TokenType::Struct => self.struct_declaration(),
            TokenType::Identifier(_) if self.peek_next_is_ident() => self.var_declaration(),
            _ => self.statement(),
        }
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

    fn statement(&mut self) -> Stmt {
        match self.peek() {
            TokenType::If => self.if_statement(),
            TokenType::While => self.while_statement(),
            TokenType::LBrace => {
                self.advance();
                Stmt::Block(self.block())
            },
            TokenType::Return => {
                self.advance(); // consume 'return'
                let value = self.expression();
                self.consume(TokenType::Semicolon, "Expect ';' after return value.");
                Stmt::Return(value)
            }
            _ => {
                let expr = self.expression();
                self.consume(TokenType::Semicolon, "Expect ';' after expression.");
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
                    Expr::FieldAccess { object: Box::new(left), field }
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
        let name = if let Expr::Variable(n) = left {
            n
        } else {
            panic!("[Line {}] Invalid assignment target.", self.peek_line());
        };

        let value = self.parse_precedence(Precedence::Assignment);

        Expr::Assign {
            name,
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

       
    fn peek_next_is_ident(&self) -> bool {
        matches!(self.tokens.get(self.current + 1).map(|t| &t.kind), 
            Some(TokenType::Identifier(_)))
    }

    fn parse_type(&mut self) -> String {
        match self.advance().clone() {
            TokenType::Identifier(n) => n,
            other => format!("{:?}", other), // Int, Float, Bool, Str, Void
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
