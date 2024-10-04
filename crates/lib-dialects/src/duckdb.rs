use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::init::DialectKind;
use sqruff_lib_core::dialects::syntax::SyntaxKind;
use sqruff_lib_core::helpers::{Config, ToMatchable};
use sqruff_lib_core::parser::grammar::anyof::one_of;
use sqruff_lib_core::parser::grammar::base::Ref;
use sqruff_lib_core::parser::grammar::delimited::Delimited;
use sqruff_lib_core::parser::grammar::sequence::{Bracketed, Sequence};
use sqruff_lib_core::parser::lexer::Matcher;
use sqruff_lib_core::parser::matchable::MatchableTrait;
use sqruff_lib_core::parser::parsers::StringParser;
use sqruff_lib_core::parser::segments::meta::MetaSegment;
use sqruff_lib_core::vec_of_erased;

pub fn dialect() -> Dialect {
    raw_dialect().config(|dialect| dialect.expand())
}

pub fn raw_dialect() -> Dialect {
    let ansi_dialect = super::ansi::raw_dialect();
    let postgres_dialect = super::postgres::dialect();
    let mut duckdb_dialect = postgres_dialect;
    duckdb_dialect.name = DialectKind::Duckdb;

    duckdb_dialect.add([
        (
            "SingleIdentifierGrammar".into(),
            one_of(vec_of_erased![
                Ref::new("NakedIdentifierSegment"),
                Ref::new("QuotedIdentifierSegment"),
                Ref::new("SingleQuotedIdentifierSegment")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "DivideSegment".into(),
            one_of(vec_of_erased![
                StringParser::new("//", SyntaxKind::BinaryOperator),
                StringParser::new("/", SyntaxKind::BinaryOperator)
            ])
            .to_matchable()
            .into(),
        ),
        (
            "UnionGrammar".into(),
            ansi_dialect
                .grammar("UnionGrammar")
                .copy(
                    Some(vec_of_erased![Sequence::new(vec_of_erased![
                        Ref::keyword("BY"),
                        Ref::keyword("NAME")
                    ])
                    .config(|this| this.optional())]),
                    None,
                    None,
                    None,
                    Vec::new(),
                    false,
                )
                .into(),
        ),
    ]);

    duckdb_dialect.insert_lexer_matchers(
        vec![Matcher::string(
            "double_divide",
            "//",
            SyntaxKind::DoubleDivide,
        )],
        "divide",
    );

    duckdb_dialect.replace_grammar(
        "SelectClauseElementSegment",
        one_of(vec_of_erased![
            Sequence::new(vec_of_erased![
                Ref::new("WildcardExpressionSegment"),
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("EXCLUDE"),
                        one_of(vec_of_erased![
                            Ref::new("ColumnReferenceSegment"),
                            Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                                Ref::new("ColumnReferenceSegment")
                            ])])
                        ])
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("REPLACE"),
                        Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                            Sequence::new(vec_of_erased![
                                Ref::new("BaseExpressionElementGrammar"),
                                Ref::new("AliasExpressionSegment").optional()
                            ])
                        ])])
                    ])
                ])
                .config(|config| {
                    config.optional();
                })
            ]),
            Sequence::new(vec_of_erased![
                Ref::new("BaseExpressionElementGrammar"),
                Ref::new("AliasExpressionSegment").optional()
            ])
        ])
        .to_matchable(),
    );

    duckdb_dialect.replace_grammar(
        "OrderByClauseSegment",
        Sequence::new(vec_of_erased![
            Ref::keyword("ORDER"),
            Ref::keyword("BY"),
            MetaSegment::indent(),
            Delimited::new(vec_of_erased![Sequence::new(vec_of_erased![
                one_of(vec_of_erased![
                    Ref::keyword("ALL"),
                    Ref::new("ColumnReferenceSegment"),
                    Ref::new("NumericLiteralSegment"),
                    Ref::new("ExpressionSegment")
                ]),
                one_of(vec_of_erased![Ref::keyword("ASC"), Ref::keyword("DESC")]).config(
                    |config| {
                        config.optional();
                    }
                ),
                Sequence::new(vec_of_erased![
                    Ref::keyword("NULLS"),
                    one_of(vec_of_erased![Ref::keyword("FIRST"), Ref::keyword("LAST")])
                ])
                .config(|config| {
                    config.optional();
                })
            ])])
            .config(|config| {
                config.allow_trailing = true;
                config.terminators = vec_of_erased![Ref::new("OrderByClauseTerminators")];
            }),
            MetaSegment::dedent()
        ])
        .to_matchable(),
    );

    duckdb_dialect.replace_grammar(
        "GroupByClauseSegment",
        Sequence::new(vec_of_erased![
            Ref::keyword("GROUP"),
            Ref::keyword("BY"),
            MetaSegment::indent(),
            Delimited::new(vec_of_erased![one_of(vec_of_erased![
                Ref::keyword("ALL"),
                Ref::new("ColumnReferenceSegment"),
                Ref::new("NumericLiteralSegment"),
                Ref::new("ExpressionSegment")
            ])])
            .config(|config| {
                config.allow_trailing = true;
                config.terminators = vec_of_erased![Ref::new("GroupByClauseTerminatorGrammar")];
            }),
            MetaSegment::dedent()
        ])
        .to_matchable(),
    );

    duckdb_dialect.replace_grammar(
        "ObjectLiteralElementSegment",
        Sequence::new(vec_of_erased![
            one_of(vec_of_erased![
                Ref::new("NakedIdentifierSegment"),
                Ref::new("QuotedLiteralSegment")
            ]),
            Ref::new("ColonSegment"),
            Ref::new("BaseExpressionElementGrammar")
        ])
        .to_matchable(),
    );

    duckdb_dialect
}