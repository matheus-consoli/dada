use crate::{parser::Parser, token_test::Identifier};

use dada_id::InternValue;
use dada_ir::{
    code::{
        Ast, Block, BlockData, Expr, ExprData, NamedExpr, NamedExprSpan, PushSpan, Spans, Tables,
    },
    kw::Keyword,
    op::Op,
    token::Token,
    token_tree::TokenTree,
};
use salsa::AsId;

use super::OrReportError;

impl Parser<'_> {
    pub(crate) fn parse_ast(&mut self) -> Ast {
        let mut tables = Tables::default();
        let mut spans = Spans::default();

        let mut code_parser = CodeParser {
            parser: self,
            tables: &mut tables,
            spans: &mut spans,
        };

        let block = code_parser.parse_only_block_contents();
        Ast { tables, block }
    }
}

struct CodeParser<'me, 'db> {
    parser: &'me mut Parser<'db>,
    tables: &'me mut Tables,
    spans: &'me mut Spans,
}

impl<'db> std::ops::Deref for CodeParser<'_, 'db> {
    type Target = Parser<'db>;

    fn deref(&self) -> &Self::Target {
        &self.parser
    }
}

impl<'db> std::ops::DerefMut for CodeParser<'_, 'db> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.parser
    }
}

impl CodeParser<'_, '_> {
    /// Parses a series of expressions; expects to consume all available tokens (and errors if there are extra).
    pub(crate) fn parse_only_block_contents(&mut self) -> Block {
        let mut exprs = vec![];
        while self.tokens.peek().is_some() {
            if let Some(expr) = self.parse_expr() {
                exprs.push(expr);
            } else {
                self.report_error_at_current_token("expected expression");
                self.tokens.consume();
            }
        }
        self.tables.add(BlockData { exprs })
    }

    /// Parses a series of named expressions (`id: expr`); expects to consume all available tokens (and errors if there are extra).
    pub(crate) fn parse_only_named_exprs(&mut self) -> Vec<NamedExpr> {
        let mut exprs = vec![];
        while self.tokens.peek().is_some() {
            if let Some(expr) = self.parse_named_expr() {
                exprs.push(expr);
            } else {
                self.report_error_at_current_token("expected expression");
                self.tokens.consume();
            }
        }
        exprs
    }

    /// Parses a single expression (and errors if there are extra tokens)
    pub(crate) fn parse_only_expr(&mut self) -> Expr {
        if let Some(expr) = self.parse_expr() {
            if self.tokens.peek().is_some() {
                self.report_error_at_current_token("extra tokens after expression");
            }

            return expr;
        }

        self.report_error_at_current_token("expected expression");
        None.or_dummy_expr(self)
    }

    fn add<D, K>(&mut self, data: D, span: K::Span) -> K
    where
        D: std::hash::Hash + Eq,
        Tables: InternValue<D, Key = K>,
        K: PushSpan + AsId,
    {
        let key = self.tables.add(data);
        key.push_span(&mut self.spans, span);
        key
    }

    fn parse_required_expr(&mut self, before: impl std::fmt::Display) -> Expr {
        self.parse_expr()
            .or_report_error(self, || format!("expected expression after {before}"))
            .or_dummy_expr(self)
    }

    /// Parses an if/while condition -- this can be any sort of expression but a block.
    pub(crate) fn parse_condition(&mut self) -> Option<Expr> {
        if self.peek_if(Token::Delimiter('{')).is_some() {
            None
        } else {
            self.parse_expr()
        }
    }

    ///
    pub(crate) fn parse_named_expr(&mut self) -> Option<NamedExpr> {
        let (id_span, id) = self
            .eat_if(Identifier)
            .or_report_error(self, || format!("expected name for argument"))?;

        self.eat_op(Op::Colon)
            .or_report_error(self, || format!("expected `:` after argument name"));

        let expr = self
            .parse_expr()
            .or_report_error(self, || format!("expected expression"))
            .or_dummy_expr(self);

        Some(self.add(
            dada_ir::code::NamedExprData { name: id, expr },
            NamedExprSpan {
                span: self.span_consumed_since(id_span),
                name_span: id_span,
            },
        ))
    }

    /// ```
    /// Expr := Id
    ///       | UnaryOp Expr
    ///       | `if` Expr Block [`else` Block]
    ///       | `while` Expr Block
    ///       | `loop` Block
    ///       | `continue`
    ///       | `break` [Expr]
    ///       | `return` [Expr]
    ///       | Block
    ///       | Expr . Ident
    ///       | Expr BinaryOp Expr
    ///       | Expr ( args )
    /// ```
    pub(crate) fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_expr_3()
    }

    pub(crate) fn parse_expr_3(&mut self) -> Option<Expr> {
        let expr = self.parse_expr_2()?;

        if let Some(expr1) = self.parse_binop(expr, &[Op::Plus, Op::Minus]) {
            return Some(expr1);
        }

        Some(expr)
    }

    pub(crate) fn parse_expr_2(&mut self) -> Option<Expr> {
        let expr = self.parse_expr_1()?;

        if let Some(expr1) = self.parse_binop(expr, &[Op::DividedBy, Op::Times]) {
            return Some(expr1);
        }

        Some(expr)
    }

    pub(crate) fn parse_expr_1(&mut self) -> Option<Expr> {
        let expr = self.parse_expr_0()?;

        if let Some(_) = self.eat_op(Op::Dot) {
            if let Some((id_span, id)) = self.eat_if(Identifier) {
                let span = self.spans[expr].to(id_span);
                return Some(self.add(ExprData::Dot(expr, id), span));
            } else {
                self.parser
                    .report_error_at_current_token("expected identifier after `.`");
            }
        }

        if let Some((arg_span, token_tree)) = self.delimited('(') {
            // `base(...)`
            let named_exprs =
                self.with_sub_parser(token_tree, |sub_parser| sub_parser.parse_only_named_exprs());
            let span = self.spans[expr].to(arg_span);
            return Some(self.add(ExprData::Call(expr, named_exprs), span));
        }

        Some(expr)
    }

    pub(crate) fn parse_expr_0(&mut self) -> Option<Expr> {
        if let Some((id_span, id)) = self.eat_if(Identifier) {
            Some(self.add(ExprData::Id(id), id_span))
        } else if let Some(expr) = self.parse_block_expr() {
            // { ... }
            Some(expr)
        } else if let Some((if_span, _)) = self.eat_if(Keyword::If) {
            if let Some(condition) = self.parse_condition() {
                let then_expr = self.parse_required_block_expr(Keyword::If);
                let else_expr = self
                    .eat_if(Keyword::Else)
                    .map(|_| self.parse_required_block_expr(Keyword::Else));
                let span = self.span_consumed_since(if_span);
                Some(self.add(ExprData::If(condition, then_expr, else_expr), span))
            } else {
                self.report_error_at_current_token("expected `if` condition");
                None
            }
        } else if let Some((while_span, _)) = self.eat_if(Keyword::While) {
            if let Some(condition) = self.parse_condition() {
                let body = self.parse_required_block_expr(Keyword::While);
                let span = self.span_consumed_since(while_span);
                Some(self.add(ExprData::While(condition, body), span))
            } else {
                self.report_error_at_current_token("expected `while` condition");
                None
            }
        } else if let Some((span, token_tree)) = self.delimited('(') {
            let expr = self.with_sub_parser(token_tree, |subparser| subparser.parse_only_expr());
            Some(self.add(ExprData::Parenthesized(expr), span))
        } else {
            None
        }
    }

    fn parse_required_block_expr(&mut self, after: impl std::fmt::Display) -> Expr {
        self.parse_block_expr()
            .or_report_error(self, || format!("expected block after {after}"))
            .or_dummy_expr(self)
    }

    fn parse_block_expr(&mut self) -> Option<Expr> {
        let (span, token_tree) = self.delimited('{')?;
        let block = self.with_sub_parser(token_tree, |sub_parser| {
            sub_parser.parse_only_block_contents()
        });
        let expr = self.add(ExprData::Block(block), span);
        Some(expr)
    }

    fn parse_binop(&mut self, base: Expr, ops: &[Op]) -> Option<Expr> {
        for &op in ops {
            if let Some(_) = self.eat_op(op) {
                let rhs = self.parse_required_expr(op);
                let span = self.spans[base].to(self.spans[rhs]);
                return Some(self.add(ExprData::Op(base, op, rhs), span));
            }
        }
        None
    }

    fn with_sub_parser<R>(
        &mut self,
        token_tree: TokenTree,
        op: impl FnOnce(&mut CodeParser<'_, '_>) -> R,
    ) -> R {
        let mut parser = Parser::new(self.db, token_tree, &mut self.parser.errors);
        let mut sub_parser = CodeParser {
            parser: &mut parser,
            tables: &mut self.tables,
            spans: &mut self.spans,
        };
        stacker::maybe_grow(32 * 1024, 1024 * 1024, || op(&mut sub_parser))
    }
}

trait OrDummyExpr {
    fn or_dummy_expr(self, parser: &mut CodeParser<'_, '_>) -> Expr;
}

impl OrDummyExpr for Option<Expr> {
    fn or_dummy_expr(self, parser: &mut CodeParser<'_, '_>) -> Expr {
        self.unwrap_or_else(|| parser.add(ExprData::Error, parser.tokens.peek_span()))
    }
}