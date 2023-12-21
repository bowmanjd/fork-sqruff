fn trim_to_terminator(
    segments: Vec<Box<dyn Segment>>,
    tail: Vec<Box<dyn Segment>>,
    terminators: Vec<Box<dyn Matchable>>,
    parse_context: &mut ParseContext,
) -> Result<(Vec<Box<dyn Segment>>, Vec<Box<dyn Segment>>), SQLParseError> {
    let term_match =
        parse_context.deeper_match("Sequence-GreedyB-@0", false, &[], false.into(), |this| {
            greedy_match(segments.clone(), this, terminators, false)
        })?;

    if term_match.has_match() {
        // If we _do_ find a terminator, we separate off everything
        // beyond that terminator (and any preceding non-code) so that
        // it's not available to match against for the rest of this.
        let tail = &term_match.unmatched_segments;
        let segments = &term_match.matched_segments;

        for (idx, segment) in segments.iter().enumerate().rev() {
            if segment.is_code() {
                return Ok(split_and_concatenate(segments, idx, tail));
            }
        }
    }

    Ok((segments.clone(), tail.clone()))
}

fn split_and_concatenate<T>(segments: &[T], idx: usize, tail: &[T]) -> (Vec<T>, Vec<T>)
where
    T: Clone,
{
    let first_part = segments[..idx + 1].to_vec();
    let second_part = segments[idx + 1..].iter().chain(tail).cloned().collect();

    (first_part, second_part)
}

fn position_metas(
    metas: &[Indent],              // Assuming Indent is a struct or type alias
    non_code: &[Box<dyn Segment>], // Assuming BaseSegment is a struct or type alias
) -> Vec<Box<dyn Segment>> {
    // Assuming BaseSegment can be cloned, or you have a way to handle ownership transfer

    // Check if all metas have a non-negative indent value
    if metas.iter().all(|m| m.indent_val >= 0) {
        let mut result: Vec<Box<dyn Segment>> = Vec::new();

        // Append metas first, then non-code elements
        for meta in metas {
            result.push(meta.clone().boxed()); // Assuming clone is possible or some equivalent
        }
        for segment in non_code {
            result.push(segment.clone()); // Assuming clone is possible or some equivalent
        }

        result
    } else {
        let mut result: Vec<Box<dyn Segment>> = Vec::new();

        // Append non-code elements first, then metas
        for segment in non_code {
            result.push(segment.clone()); // Assuming clone is possible or some equivalent
        }
        for meta in metas {
            result.push(meta.clone().boxed()); // Assuming clone is possible or some equivalent
        }

        result
    }
}

use std::{collections::HashSet, iter::zip};

use itertools::{chain, enumerate, Itertools};

use crate::{
    core::{
        errors::SQLParseError,
        parser::{
            context::ParseContext,
            helpers::trim_non_code_segments,
            match_algorithms::{bracket_sensitive_look_ahead_match, greedy_match},
            match_result::MatchResult,
            matchable::Matchable,
            segments::{base::Segment, meta::Indent},
            types::ParseMode,
        },
    },
    helpers::Boxed,
};

#[derive(Debug, Clone)]
pub struct Sequence {
    elements: Vec<Box<dyn Matchable>>,
    parse_mode: ParseMode,
    allow_gaps: bool,
    is_optional: bool,
    terminators: Vec<Box<dyn Matchable>>,
}

impl Sequence {
    pub fn new(elements: Vec<Box<dyn Matchable>>) -> Self {
        Self {
            elements,
            allow_gaps: true,
            is_optional: false,
            parse_mode: ParseMode::Strict,
            terminators: Vec::new(),
        }
    }

    pub fn terminators(mut self, terminators: Vec<Box<dyn Matchable>>) -> Self {
        self.terminators = terminators;
        self
    }

    pub fn parse_mode(mut self, mode: ParseMode) -> Self {
        self.parse_mode = mode;
        self
    }

    pub fn allow_gaps(mut self, allow_gaps: bool) -> Self {
        self.allow_gaps = allow_gaps;
        self
    }
}

impl PartialEq for Sequence {
    fn eq(&self, other: &Self) -> bool {
        zip(&self.elements, &other.elements).all(|(a, b)| a.dyn_eq(&*b.clone()))
    }
}

impl Segment for Sequence {}

impl Matchable for Sequence {
    fn is_optional(&self) -> bool {
        self.is_optional
    }

    // Does this matcher support a uppercase hash matching route?
    //
    // Sequence does provide this, as long as the *first* non-optional
    // element does, *AND* and optional elements which preceded it also do.
    fn simple(
        &self,
        parse_context: &ParseContext,
        crumbs: Option<Vec<&str>>,
    ) -> Option<(HashSet<String>, HashSet<String>)> {
        let mut simple_raws = HashSet::new();
        let mut simple_types = HashSet::new();

        for opt in &self.elements {
            let Some((raws, types)) = opt.simple(parse_context, crumbs.clone()) else {
                return None;
            };

            simple_raws.extend(raws);
            simple_types.extend(types);

            if !opt.is_optional() {
                // We found our first non-optional element!
                return (simple_raws, simple_types).into();
            }
        }

        // If *all* elements are optional AND simple, I guess it's also simple.
        (simple_raws, simple_types).into()
    }

    fn match_segments(
        &self,
        segments: Vec<Box<dyn Segment>>,
        parse_context: &mut ParseContext,
    ) -> Result<MatchResult, SQLParseError> {
        let mut matched_segments = Vec::new();
        let mut unmatched_segments = segments.clone();
        let mut tail = Vec::new();
        let mut first_match = true;

        // Buffers of segments, not yet added.
        let mut meta_buffer = Vec::new();
        let mut non_code_buffer = Vec::new();

        for (idx, elem) in enumerate(&self.elements) {
            // 1. Handle any metas or conditionals.
            // We do this first so that it's the same whether we've run
            // out of segments or not.
            // If it's a conditional, evaluate it.
            // In both cases, we don't actually add them as inserts yet
            // because their position will depend on what types we accrue.
            if let Some(indent) = elem.as_any().downcast_ref::<Indent>() {
                meta_buffer.push(indent.clone());
                continue;
            }

            // 2. Handle any gaps in the sequence.
            // At this point we know the next element isn't a meta or conditional
            // so if we're going to look for it we need to work up to the next
            // code element (if allowed)
            if self.allow_gaps && !matched_segments.is_empty() {
                // First, if we're allowing gaps, consume any non-code.
                // NOTE: This won't consume from the end of a sequence
                // because this happens only in the run up to matching
                // another element. This is as designed. It also won't
                // happen at the *start* of a sequence either.

                for (idx, segment) in unmatched_segments.iter().enumerate() {
                    if segment.is_code() {
                        non_code_buffer.extend_from_slice(&unmatched_segments[..idx]);
                        unmatched_segments = unmatched_segments[idx..].to_vec();

                        break;
                    }
                }
            }

            // 4. Match the current element against the current position.
            let elem_match = parse_context.deeper_match(
                format!("Sequence-@{idx}"),
                false,
                &[],
                None,
                |this| elem.match_segments(unmatched_segments.clone(), this),
            )?;

            if !elem_match.has_match() {
                // If we can't match an element, we should ascertain whether it's
                // required. If so then fine, move on, but otherwise we should
                // crash out without a match. We have not matched the sequence.
                if elem.is_optional() {
                    // Pass this one and move onto the next element.
                    continue;
                }

                if self.parse_mode == ParseMode::Strict {
                    // In a strict mode, failing to match an element means that
                    // we don't match anything.
                    return Ok(MatchResult::from_unmatched(segments));
                }
            }

            // 5. Successful match: Update the buffers.
            // First flush any metas along with the gap.
            let segments = position_metas(&meta_buffer, &non_code_buffer);
            matched_segments.extend(segments);
            non_code_buffer = Vec::new();
            meta_buffer = Vec::new();

            // Add on the match itself
            matched_segments.extend(elem_match.matched_segments);
            unmatched_segments = elem_match.unmatched_segments;
            // parse_context.update_progress(matched_segments)

            if first_match && self.parse_mode == ParseMode::GreedyOnceStarted {
                // In the GREEDY_ONCE_STARTED mode, we first look ahead to find a
                // terminator after the first match (and only the first match).
                let mut terminators = parse_context.terminators.clone();
                terminators.extend(self.terminators.clone());

                (unmatched_segments, tail) = trim_to_terminator(
                    unmatched_segments.clone(),
                    tail.clone(),
                    terminators,
                    parse_context,
                )?;

                first_match = false;
            }
        }

        // If we finished on an optional, and so still have some unflushed metas,
        // we should do that first, then add any unmatched noncode back onto the
        // unmatched sequence.
        if !meta_buffer.is_empty() {
            matched_segments.extend(
                meta_buffer
                    .into_iter()
                    .map(|it| it.boxed() as Box<dyn Segment>),
            );
        }

        if !non_code_buffer.is_empty() {
            unmatched_segments = chain(non_code_buffer, unmatched_segments).collect_vec();
        }

        // If we get to here, we've matched all of the elements (or skipped them).
        // Return successfully.
        unmatched_segments.extend(tail);

        Ok(MatchResult {
            matched_segments,
            unmatched_segments,
        })
    }

    fn cache_key(&self) -> String {
        todo!()
    }

    fn copy(
        &self,
        insert: Option<Vec<Box<dyn Matchable>>>,
        replace_terminators: bool,
        terminators: Vec<Box<dyn Matchable>>,
    ) -> Box<dyn Matchable> {
        let mut new_elems = self.elements.clone();

        if let Some(insert) = insert {
            new_elems.extend(insert);
        }

        let mut new_grammar = self.clone();
        new_grammar.elements = new_elems;

        if replace_terminators {
            new_grammar.terminators = terminators;
        } else {
            new_grammar.terminators.extend(terminators);
        }

        new_grammar.boxed()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bracketed {
    bracket_type: &'static str,
    bracket_pairs_set: &'static str,
    allow_gaps: bool,

    this: Sequence,
}

impl Bracketed {
    pub fn new(args: Vec<Box<dyn Matchable>>) -> Self {
        Self {
            bracket_type: "round",
            bracket_pairs_set: "bracket_pairs",
            allow_gaps: true,
            this: Sequence::new(args),
        }
    }
}

impl Bracketed {
    pub fn bracket_type(mut self, bracket_type: &'static str) -> Self {
        self.bracket_type = bracket_type;
        self
    }

    fn get_bracket_from_dialect(
        &self,
        parse_context: &ParseContext,
    ) -> Result<(Box<dyn Matchable>, Box<dyn Matchable>, bool), String> {
        // Assuming bracket_pairs_set and other relevant fields are part of self
        let bracket_pairs = parse_context.dialect().bracket_sets(self.bracket_pairs_set);
        for (bracket_type, start_ref, end_ref, persists) in bracket_pairs {
            if bracket_type == self.bracket_type {
                let start_bracket = parse_context.dialect().r#ref(&start_ref);
                let end_bracket = parse_context.dialect().r#ref(&end_ref);

                return Ok((start_bracket, end_bracket, persists));
            }
        }
        Err(format!(
            "bracket_type {:?} not found in bracket_pairs of {:?} dialect.",
            self.bracket_type,
            parse_context.dialect()
        ))
    }
}

impl Segment for Bracketed {}

impl Matchable for Bracketed {
    fn simple(
        &self,
        parse_context: &ParseContext,
        crumbs: Option<Vec<&str>>,
    ) -> Option<(HashSet<String>, HashSet<String>)> {
        let (start_bracket, _, _) = self.get_bracket_from_dialect(parse_context).unwrap();
        start_bracket.simple(parse_context, crumbs)
    }

    fn match_segments(
        &self,
        segments: Vec<Box<dyn Segment>>,
        parse_context: &mut ParseContext,
    ) -> Result<MatchResult, SQLParseError> {
        enum Status {
            Matched(MatchResult, Vec<Box<dyn Segment>>),
            EarlyReturn(MatchResult),
            Fail(SQLParseError),
        }

        // Trim ends if allowed.
        let mut seg_buff = if self.allow_gaps {
            let (_, seg_buff, _) = trim_non_code_segments(&segments);
            seg_buff.to_vec()
        } else {
            segments.clone()
        };

        // Rehydrate the bracket segments in question.
        // bracket_persists controls whether we make a BracketedSegment or not.
        let (start_bracket, end_bracket, bracket_persists) =
            self.get_bracket_from_dialect(parse_context).unwrap();

        // Allow optional override for special bracket-like things
        let start_bracket = start_bracket;
        let end_bracket = end_bracket;

        if seg_buff
            .last()
            .map_or(false, |seg| seg.is_type("bracketed"))
        {
            unimplemented!()
        } else {
            // Look for the first bracket
            let status = parse_context.deeper_match("Bracketed-First", false, &[], None, |this| {
                let start_match = start_bracket.match_segments(segments.clone(), this);

                match start_match {
                    Ok(start_match) if start_match.has_match() => {
                        let unmatched_segments = start_match.unmatched_segments.clone();
                        Status::Matched(start_match, unmatched_segments)
                    }
                    Ok(_) => Status::EarlyReturn(MatchResult::from_unmatched(segments)),
                    Err(err) => Status::Fail(err),
                }
            });

            let start_match = match status {
                Status::Matched(match_result, segments) => {
                    seg_buff = segments;
                    match_result
                }
                Status::EarlyReturn(match_result) => return Ok(match_result),
                Status::Fail(error) => return Err(error),
            };

            let (content_segs, end_match) =
                parse_context.deeper_match("Bracketed-End", true, &[], None, |this| {
                    let (content_segs, end_match, _) = bracket_sensitive_look_ahead_match(
                        seg_buff,
                        vec![end_bracket.clone()],
                        this,
                        start_bracket.into(),
                        end_bracket.into(),
                        self.bracket_pairs_set.into(),
                    )?;

                    Ok((content_segs, end_match))
                })?;

            if !end_match.has_match() {
                panic!("Couldn't find closing bracket for opening bracket.")
            }

            // Then trim whitespace and deal with the case of non-code content e.g. "(   )"
            let (pre_segs, content_segs, post_segs) = if self.allow_gaps {
                trim_non_code_segments(&content_segs)
            } else {
                (&[][..], &[][..], &[][..])
            };

            let content_match =
                parse_context.deeper_match("Bracketed", true, &[], None, |this| {
                    self.this.match_segments(content_segs.to_vec(), this)
                })?;

            if !content_match.has_match() {
                panic!()
            }

            let segments = {
                let mut this = start_match.matched_segments;
                this.extend(pre_segs.to_vec());
                this.extend(end_match.matched_segments);
                this
            };

            unimplemented!()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core::parser::{
            context::ParseContext,
            markers::PositionMarker,
            matchable::Matchable,
            parsers::StringParser,
            segments::{
                keyword::KeywordSegment,
                meta::Indent,
                test_functions::{fresh_ansi_dialect, test_segments},
            },
        },
        helpers::{Boxed, ToMatchable},
    };

    use super::Sequence;

    #[test]
    fn test__parser__grammar_sequence() {
        let bs = StringParser::new(
            "bar",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let fs = StringParser::new(
            "foo",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let mut ctx = ParseContext::new(fresh_ansi_dialect());

        let g = Sequence::new(vec![bs.clone(), fs.clone()]);
        let gc = Sequence::new(vec![bs, fs]).allow_gaps(false);

        let match_result = g.match_segments(test_segments(), &mut ctx).unwrap();

        assert_eq!(match_result.matched_segments[0].get_raw().unwrap(), "bar");
        assert_eq!(
            match_result.matched_segments[1].get_raw().unwrap(),
            test_segments()[1].get_raw().unwrap()
        );
        assert_eq!(match_result.matched_segments[2].get_raw().unwrap(), "foo");
        assert_eq!(match_result.len(), 3);

        assert!(!gc
            .match_segments(test_segments(), &mut ctx)
            .unwrap()
            .has_match());

        assert!(!g
            .match_segments(test_segments()[1..].to_vec(), &mut ctx)
            .unwrap()
            .has_match());
    }

    #[test]
    fn test__parser__grammar_sequence_nested() {
        let bs = StringParser::new(
            "bar",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let fs = StringParser::new(
            "foo",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let bas = StringParser::new(
            "baar",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let g = Sequence::new(vec![Sequence::new(vec![bs, fs]).boxed(), bas]);

        let mut ctx = ParseContext::new(fresh_ansi_dialect());

        assert!(
            !g.match_segments(test_segments()[..2].to_vec(), &mut ctx)
                .unwrap()
                .has_match(),
            "Expected no match, but a match was found."
        );

        let segments = g
            .match_segments(test_segments(), &mut ctx)
            .unwrap()
            .matched_segments;
        assert_eq!(segments[0].get_raw().unwrap(), "bar");
        assert_eq!(
            segments[1].get_raw().unwrap(),
            test_segments()[1].get_raw().unwrap()
        );
        assert_eq!(segments[2].get_raw().unwrap(), "foo");
        assert_eq!(segments[3].get_raw().unwrap(), "baar");
        assert_eq!(segments.len(), 4);
    }

    #[test]
    fn test__parser__grammar_sequence_indent() {
        let bs = StringParser::new(
            "bar",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let fs = StringParser::new(
            "foo",
            |segment| {
                KeywordSegment::new(
                    segment.get_raw().unwrap(),
                    segment.get_position_marker().unwrap(),
                )
                .boxed()
            },
            None,
            false,
            None,
        )
        .boxed();

        let g = Sequence::new(vec![
            Indent::new(PositionMarker::default()).to_matchable(),
            bs,
            fs,
        ]);
        let mut ctx = ParseContext::new(fresh_ansi_dialect());
        let segments = g
            .match_segments(test_segments(), &mut ctx)
            .unwrap()
            .matched_segments;

        assert_eq!(segments[0].get_type(), "indent");
        assert_eq!(segments[1].get_type(), "kw");
    }
}
