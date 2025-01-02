use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::init::DialectKind;
use sqruff_lib_core::helpers::Config;
/*
use sqruff_lib_core::helpers::ToMatchable;
use sqruff_lib_core::dialects::syntax::SyntaxKind;
use sqruff_lib_core::parser::grammar::anyof::{one_of, AnyNumberOf};
use sqruff_lib_core::parser::grammar::base::{Anything, Nothing, Ref};
use sqruff_lib_core::parser::grammar::delimited::Delimited;
use sqruff_lib_core::parser::grammar::sequence::{Bracketed, Sequence};
use sqruff_lib_core::parser::matchable::MatchableTrait;
use sqruff_lib_core::parser::node_matcher::NodeMatcher;
use sqruff_lib_core::parser::parsers::TypedParser;
use sqruff_lib_core::parser::segments::meta::MetaSegment;
use sqruff_lib_core::vec_of_erased;
*/

pub fn dialect() -> Dialect {
    let ansi_dialect = super::ansi::raw_dialect();
    let mut tsql_dialect = ansi_dialect;
    tsql_dialect.name = DialectKind::Trino;

    tsql_dialect.sets_mut("bare_functions").extend([
        "current_timestamp",
        "current_user",
        "session_user",
        "system_user",
    ]);

    tsql_dialect.sets_mut("unreserved_keywords").clear();
    tsql_dialect.update_keywords_set_from_multiline_string(
        "unreserved_keywords",
        super::tsql_keywords::TSQL_UNRESERVED_KEYWORDS,
    );

    tsql_dialect.sets_mut("reserved_keywords").clear();
    tsql_dialect.update_keywords_set_from_multiline_string(
        "reserved_keywords",
        super::tsql_keywords::TSQL_RESERVED_KEYWORDS,
    );

    tsql_dialect.sets_mut("datetime_units").extend([
        "D",
        "DAY",
        "DAYS",
        "DAYOFYEAR",
        "DD",
        "DW",
        "DY",
        "HH",
        "HOUR",
        "ISO_WEEK",
        "ISOWK",
        "ISOWW",
        "INFINITE",
        "M",
        "MCS",
        "MI",
        "MICROSECOND",
        "MILLISECOND",
        "MINUTE",
        "MM",
        "MONTH",
        "MONTHS",
        "MS",
        "N",
        "NANOSECOND",
        "NS",
        "Q",
        "QQ",
        "QUARTER",
        "S",
        "SECOND",
        "SS",
        "TZ",
        "TZOFFSET",
        "W",
        "WEEK",
        "WEEKS",
        "WEEKDAY",
        "WK",
        "WW",
        "YEAR",
        "YEARS",
        "Y",
        "YY",
        "YYYY",
    ]);

    tsql_dialect.sets_mut("date_part_function_name").clear();
    tsql_dialect.sets_mut("date_part_function_name").extend([
        "DATEADD",
        "DATEDIFF",
        "DATEDIFF_BIG",
        "DATENAME",
        "DATEPART",
        "DATETRUNC",
    ]);

    tsql_dialect.config(|dialect| dialect.expand())
}
