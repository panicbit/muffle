use chumsky::Parser;
use regex::Regex;
use serde::{Deserialize, Deserializer, de};

use crate::parsing;

mod parser;
use parser::parser;

pub struct Context<'a> {
    pub output_name: &'a str,
    pub input_name: &'a str,
}

#[derive(Debug)]
pub enum Expr {
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
    PortFilter(PortFilter),
}

impl Expr {
    pub fn eval(&self, context: &Context<'_>) -> bool {
        match self {
            Expr::And(lhs, rhs) => lhs.eval(context) && rhs.eval(context),
            Expr::Or(lhs, rhs) => lhs.eval(context) || rhs.eval(context),
            Expr::Not(expr) => !expr.eval(context),
            Expr::PortFilter(filter) => filter.eval(context),
        }
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let expr = String::deserialize(de)?;
        let expr = parser()
            .parse(&expr)
            .into_result()
            .map_err(|errs| de::Error::custom(parsing::format_errors(&errs, &expr)))?;

        Ok(expr)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Port {
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct PortFilter {
    port: Port,
    regex: Regex,
}

impl PortFilter {
    fn eval(&self, context: &Context<'_>) -> bool {
        let name = match self.port {
            Port::Input => &context.input_name,
            Port::Output => &context.output_name,
        };

        self.regex.is_match(name)
    }
}
