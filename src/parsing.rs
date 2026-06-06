use std::fmt::Display;

use ariadne::{Label, Report, ReportKind, Source};
use chumsky::Parser;
use chumsky::error::Rich;

pub type Extra<'a> = chumsky::extra::Err<chumsky::error::Rich<'a, char>>;
pub trait P<'a, O>: Parser<'a, &'a str, O, Extra<'a>> {}

impl<'a, T, O> P<'a, O> for T where T: Parser<'a, &'a str, O, Extra<'a>> {}

pub fn format_errors<E>(errs: &[Rich<'_, E>], input: &str) -> String
where
    E: Display,
{
    let mut output = Vec::new();

    for err in errs {
        let span = err.span().into_range();

        Report::build(ReportKind::Error, span.clone())
            .with_message("Failed to parse")
            .with_label(Label::new(span).with_message(err.reason()))
            .finish()
            .write_for_stdout(Source::from(input), &mut output)
            .unwrap();
    }

    String::from_utf8_lossy(&output).into_owned()
}
