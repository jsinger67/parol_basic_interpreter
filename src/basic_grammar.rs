use crate::{
    basic_grammar_trait::*,
    errors::BasicError,
    operators::{BinaryOperator, UnaryOperator},
};
use log::trace;
#[allow(unused_imports)]
use miette::{miette, Result, WrapErr};
use parol_runtime::{errors::FileSource, lexer::Token};
use std::{
    collections::BTreeMap,
    fmt::{Debug, Display, Error, Formatter},
    marker::PhantomData,
    path::{Path, PathBuf},
};

///
/// The value range for the supported calculations
///
pub type DefinitionRange = f32;

///
/// The value range for line numbers
///
pub type LineNumberRange = u16;

const MAX_LINE_NUMBER: u16 = 63999;

#[derive(Debug)]
struct CompiledLine<'a, 't>
where
    't: 'a,
{
    statements: Vec<&'a Statement<'t>>,
    next_line: Option<LineNumberRange>,
}

#[derive(Debug, Default)]
pub struct BasicLines<'a, 't> {
    lines: BTreeMap<u16, CompiledLine<'a, 't>>,
}

///
/// Data structure that implements the semantic actions for our Basic grammar
///
#[derive(Debug, Default)]
pub struct BasicGrammar<'t> {
    pub env: BTreeMap<String, DefinitionRange>,
    file_name: PathBuf,
    next_line: Option<LineNumberRange>,
    phantom: PhantomData<&'t str>, // Just to hold the lifetime generated by parol
}

impl<'t> BasicGrammar<'t> {
    pub fn new() -> Self {
        BasicGrammar::default()
    }

    fn value(&self, id: &Token<'t>) -> Result<DefinitionRange> {
        let name: &str = &id.symbol[..2];
        Ok(self.env.get(name).cloned().unwrap_or_default())
    }

    fn set_value(&mut self, id: &str, context: &str, value: DefinitionRange) {
        let name: &str = &id[..2];
        if !self.env.contains_key(name) {
            trace!("set_value {}: {}", context, name);
            self.env.insert(id.to_owned(), value);
        }
    }

    fn parse_number(&self, context: &str, token: &Token<'t>) -> Result<DefinitionRange> {
        let symbol = token.symbol.replace(' ', "").replace('E', "e");
        match symbol.parse::<DefinitionRange>() {
            Ok(number) => Ok(number),
            Err(error) => Err(miette!(BasicError::ParseFloat {
                context: context.to_owned(),
                input: FileSource::try_new(self.file_name.clone())?.into(),
                token: token.into()
            }))
            .wrap_err(miette!(error)),
        }
    }

    fn parse_line_number(&self, context: &str, token: &Token<'t>) -> Result<LineNumberRange> {
        let symbol = token.symbol.replace(' ', "");
        match symbol.parse::<LineNumberRange>() {
            Ok(number) => Ok(number),
            Err(error) => Err(miette!(BasicError::ParseLineNumber {
                context: context.to_owned(),
                input: FileSource::try_new(self.file_name.clone())?.into(),
                token: token.into()
            }))
            .wrap_err(miette!(error)),
        }
    }

    fn process_basic(&mut self, basic: &Basic<'t>) -> Result<()> {
        match basic {
            Basic::Basic0(b0) => self.process_lines(&b0.line_0, &b0.basic_list_1),
            Basic::Basic1(b1) => self.process_lines(&b1.line_1, &b1.basic_list_2),
        }
    }

    fn process_lines(
        &mut self,
        first_line: &Line<'t>,
        other_lines: &[BasicList<'t>],
    ) -> Result<()> {
        let lines = self.pre_process_lines(first_line, other_lines)?;
        self.interpret(&lines)
    }

    fn pre_process_lines<'a>(
        &mut self,
        first_line: &'a Line<'t>,
        other_lines: &'a [BasicList<'t>],
    ) -> Result<BasicLines<'a, 't>> {
        let context = "pre_process_lines";

        let mut lines = BasicLines::default();
        let (k, v) = self.pre_process_line(first_line)?;
        lines.lines.insert(k, v);

        for line in other_lines {
            let (k, v) = self.pre_process_line(&line.line_1)?;
            if lines.lines.insert(k, v).is_some() {
                return Err(miette!(BasicError::LineNumberDefinedTwice {
                    context: context.to_owned(),
                    line_number: k
                }));
            }
        }

        // Add the follow relation
        self.next_line = None;
        lines.lines.iter_mut().rev().for_each(|(k, v)| {
            v.next_line = self.next_line;
            self.next_line = Some(*k);
        });

        Ok(lines)
    }

    fn pre_process_line<'a>(
        &mut self,
        line: &'a Line<'t>,
    ) -> Result<(LineNumberRange, CompiledLine<'a, 't>)> {
        let context = "pre_process_line";
        let token = &line.line_number_0.line_number_0;
        let line_number = self.parse_line_number(context, token)?;
        if line_number > MAX_LINE_NUMBER {
            return Err(miette!(BasicError::LineNumberTooLarge {
                context: context.to_owned(),
                input: FileSource::try_new(self.file_name.clone())?.into(),
                token: token.into()
            }));
        }

        // On each line there can exist multiple statements separated by colons!
        let mut statements = vec![line.statement_1.as_ref()];

        line.line_list_2.iter().for_each(|statement| {
            statements.push(statement.statement_1.as_ref());
        });

        let compiled_line = CompiledLine {
            statements,
            next_line: None,
        };

        Ok((line_number, compiled_line))
    }

    fn interpret<'a>(&mut self, lines: &BasicLines<'a, 't>) -> Result<()> {
        while self.next_line.is_some() {
            self.interpret_line(lines)?;
        }
        Ok(())
    }

    fn interpret_line<'a>(&mut self, lines: &BasicLines<'a, 't>) -> Result<()> {
        if let Some(current_line) = lines.lines.get(&self.next_line.unwrap()) {
            self.next_line = current_line.next_line;
            let mut continue_statements = true;
            for statement in &current_line.statements {
                self.interpret_statement(statement, &mut continue_statements)?;
                if !continue_statements {
                    break;
                }
            }
            Ok(())
        } else {
            bail!("Line not accessible!")
        }
    }

    fn interpret_statement<'a>(
        &mut self,
        statement: &'a Statement<'t>,
        continue_statements: &mut bool,
    ) -> Result<()> {
        *continue_statements = true;
        match statement {
            Statement::Statement12(remark) => self.process_remark(remark),
            Statement::Statement13(goto) => {
                *continue_statements = false;
                self.process_goto(goto)
            }
            Statement::Statement14(if_statement) => {
                self.process_if_statement(if_statement, continue_statements)
            }
            Statement::Statement15(assign) => self.process_assign(assign),
            Statement::Statement16(print_statement) => {
                self.process_print_statement(print_statement)
            }
            Statement::Statement17(end_statement) => {
                *continue_statements = false;
                self.process_end_statement(end_statement)
            }
        }
    }

    fn process_remark(&self, _remark: &Statement12) -> Result<()> {
        Ok(())
    }

    fn process_goto(&mut self, goto: &Statement13<'t>) -> Result<()> {
        let context = "process_goto";
        let line_number =
            self.parse_line_number(context, &goto.goto_statement_0.line_number_1.line_number_0)?;
        self.next_line = Some(line_number);
        Ok(())
    }

    fn process_if_statement<'a>(
        &mut self,
        if_statement: &'a Statement14<'t>,
        continue_statements: &mut bool,
    ) -> Result<()> {
        let context = "process_if_statement";
        *continue_statements = true;
        let predicate = self.process_expression(&*if_statement.if_statement_0.expression_2)?;
        if predicate != 0.0 {
            match &*if_statement.if_statement_0.if_body_4 {
                IfBody::IfBody25(then) => {
                    self.interpret_statement(&*then.statement_1, continue_statements)
                }
                IfBody::IfBody26(goto) => {
                    let line_number =
                        self.parse_line_number(context, &goto.line_number_1.line_number_0)?;
                    self.next_line = Some(line_number);
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    fn process_assign(&mut self, assign: &Statement15) -> Result<()> {
        let context = "process_assign";
        match &*assign.assignment_0 {
            Assignment::Assignment23(Assignment23 {
                variable_1,
                expression_4,
                ..
            }) => {
                let value = self.process_expression(&*expression_4)?;
                self.set_value(variable_1.variable_0.symbol, context, value)
            }
            Assignment::Assignment24(Assignment24 {
                variable_0,
                expression_3,
                ..
            }) => {
                let value = self.process_expression(&*expression_3)?;
                self.set_value(variable_0.variable_0.symbol, context, value)
            }
        }
        Ok(())
    }

    fn process_print_statement(&mut self, print_statement: &Statement16) -> Result<()> {
        let value = self.process_expression(&*print_statement.print_statement_0.expression_2)?;
        print!("{value}\t");
        for elem in &print_statement.print_statement_0.print_statement_list_3 {
            let value = self.process_expression(&*elem.expression_1)?;
            print!("{value}\t");
        }
        Ok(())
    }

    fn process_end_statement(&mut self, _end_statement: &Statement17) -> Result<()> {
        self.next_line = None;
        Ok(())
    }

    fn process_expression(&mut self, expression: &Expression) -> Result<DefinitionRange> {
        self.process_logical_or(&*expression.logical_or_0)
    }

    fn process_logical_or(&mut self, logical_or: &LogicalOr) -> Result<DefinitionRange> {
        let context = "process_logical_or";
        let mut result = self.process_logical_and(&logical_or.logical_and_0)?;
        for item in &logical_or.logical_or_list_1 {
            let op: BinaryOperator = item.logical_or_op_0.logical_or_op_0.symbol.try_into()?;
            let next_operand = self.process_logical_and(&item.logical_and_1)?;
            result = BinaryOperator::apply_binary_operation(result, &op, next_operand, context)?;
        }
        Ok(result)
    }

    fn process_logical_and(&mut self, logical_and: &LogicalAnd) -> Result<DefinitionRange> {
        let context = "process_logical_and";
        let mut result = self.process_logical_not(&logical_and.logical_not_0)?;
        for item in &logical_and.logical_and_list_1 {
            let op: BinaryOperator = item.logical_and_op_0.logical_and_op_0.symbol.try_into()?;
            let next_operand = self.process_logical_not(&item.logical_not_1)?;
            result = BinaryOperator::apply_binary_operation(result, &op, next_operand, context)?;
        }
        Ok(result)
    }

    fn process_logical_not(&mut self, logical_not: &LogicalNot) -> Result<DefinitionRange> {
        let context = "process_logical_not";
        match logical_not {
            LogicalNot::LogicalNot65(not) => {
                let result = self.process_relational(&*not.relational_1)?;
                let op: UnaryOperator = not.logical_not_op_0.logical_not_op_0.symbol.try_into()?;
                UnaryOperator::apply_unary_operation(&op, result, context)
            }
            LogicalNot::LogicalNot66(not) => self.process_relational(&*not.relational_0),
        }
    }

    fn process_relational(&mut self, relational: &Relational) -> Result<DefinitionRange> {
        let context = "process_relational";
        let mut result = self.process_summation(&*relational.summation_0)?;
        for item in &relational.relational_list_1 {
            let op: BinaryOperator = item.relational_op_0.relational_op_0.symbol.try_into()?;
            let next_operand = self.process_summation(&*item.summation_1)?;
            result = BinaryOperator::apply_binary_operation(result, &op, next_operand, context)?;
        }
        Ok(result)
    }

    fn process_summation(&mut self, summation: &Summation) -> Result<DefinitionRange> {
        let context = "process_summation";
        let mut result = self.process_multiplication(&*summation.multiplication_0)?;
        for item in &summation.summation_list_1 {
            let op: BinaryOperator = match &*item.summation_list_group_0 {
                SummationListGroup::SummationListGroup72(plus) => {
                    plus.plus_0.plus_0.symbol.try_into()
                }
                SummationListGroup::SummationListGroup73(minus) => {
                    minus.minus_0.minus_0.symbol.try_into()
                }
            }?;
            let next_operand = self.process_multiplication(&*item.multiplication_1)?;
            result = BinaryOperator::apply_binary_operation(result, &op, next_operand, context)?;
        }
        Ok(result)
    }

    fn process_multiplication(
        &mut self,
        multiplication: &Multiplication,
    ) -> Result<DefinitionRange> {
        let context = "process_multiplication";
        let mut result = self.process_factor(&*multiplication.factor_0)?;
        for item in &multiplication.multiplication_list_1 {
            let op: BinaryOperator = item.mul_op_0.mul_op_0.symbol.try_into()?;
            let next_operand = self.process_factor(&*item.factor_1)?;
            result = BinaryOperator::apply_binary_operation(result, &op, next_operand, context)?;
        }
        Ok(result)
    }

    fn process_factor(&mut self, factor: &Factor) -> Result<DefinitionRange> {
        let context = "process_factor";
        match factor {
            Factor::Factor78(Factor78 { literal_0 }) => match &*literal_0.number_0 {
                Number::Number33(flt) => match &*flt.float_0 {
                    Float::Float35(float1_0) => {
                        Ok(self.parse_number(context, &float1_0.float1_0.float1_0)?)
                    }
                    Float::Float36(float2_0) => {
                        Ok(self.parse_number(context, &float2_0.float2_0.float2_0)?)
                    }
                },
                Number::Number34(int) => Ok(self.parse_number(context, &int.integer_0.integer_0)?),
            },
            Factor::Factor79(Factor79 { variable_0 }) => self.value(&variable_0.variable_0),
            Factor::Factor80(Factor80 { factor_1, .. }) => Ok(-(self.process_factor(factor_1)?)),
            Factor::Factor81(Factor81 { expression_1, .. }) => {
                self.process_expression(expression_1)
            }
        }
    }
}

impl Display for Basic<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), Error> {
        write!(f, ":-)")
    }
}

impl Display for BasicGrammar<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), Error> {
        self.env.iter().fold(Ok(()), |res, (k, v)| {
            res?;
            write!(f, "{k}: {v}")
        })
    }
}

impl<'t> BasicGrammarTrait<'t> for BasicGrammar<'t> {
    fn init(&mut self, file_name: &Path) {
        self.file_name = file_name.into();
    }

    /// Semantic action for user production 0:
    ///
    /// Basic: [EndOfLine] Line {EndOfLine Line} [EndOfLine];
    ///
    fn basic(&mut self, basic: &Basic<'t>) -> Result<()> {
        self.process_basic(basic)
    }
}
