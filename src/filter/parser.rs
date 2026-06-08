use crate::filter::{Port, PortFilter};
use crate::parsing::P;
use chumsky::Parser;
use chumsky::pratt::{infix, left, prefix};
use chumsky::prelude::*;
use regex::Regex;

use super::Expr;

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
    keyword(s).padded()
}

fn keyword<'a>(s: &'a str) -> impl P<'a, &'a str> + Clone {
    just(s).labelled(format!("\'{s}\'"))
}

fn port<'a>() -> impl P<'a, Port> + Clone {
    choice((
        keyword("input").to(Port::Input),
        keyword("output").to(Port::Output),
    ))
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
