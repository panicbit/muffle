use chumsky::Parser;
use chumsky::pratt::{infix, left, prefix};
use chumsky::prelude::*;
use regex::Regex;
use serde::{Deserialize, Deserializer, de};

use crate::parsing::{self, P};

pub struct Context<'a> {
    pub output_name: &'a str,
    pub input_name: &'a str,
}

pub fn parser<'a>() -> impl P<'a, Expr> {
    recursive(|expr| {
        let atom = choice((
            port_filter().map(Expr::PortFilter).padded(),
            just('(')
                .padded()
                .ignore_then(expr.padded())
                .then_ignore(just(')').padded()),
        ));

        atom.pratt((
            prefix(3, op("not"), |_, rhs, _| Expr::Not(Box::new(rhs))),
            infix(left(2), op("and"), |l, _, r, _| {
                Expr::And(Box::new(l), Box::new(r))
            }),
            infix(left(1), op("or"), |l, _, r, _| {
                Expr::Or(Box::new(l), Box::new(r))
            }),
        ))
    })
}

fn op<'a>(s: &'a str) -> impl P<'a, &'a str> + Clone {
    just(s).padded()
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

fn port<'a>() -> impl P<'a, Port> + Clone {
    choice((
        just("input").to(Port::Input),
        just("output").to(Port::Output),
    ))
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

fn port_filter<'a>() -> impl P<'a, PortFilter> + Clone {
    port()
        .then(regex().padded())
        .map(|(port, regex)| PortFilter { port, regex })
}

fn regex<'a>() -> impl P<'a, Regex> + Clone {
    just('/')
        .ignore_then(any().filter(|c| *c != '/').repeated().to_slice())
        .then_ignore(just('/'))
        .try_map(|pattern, span| {
            Regex::new(pattern).map_err(|err| Rich::custom(span, err.to_string()))
        })
}
