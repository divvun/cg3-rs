//! The phases of a grammar's load, as types (`docs/spec/port/src/grammar_phases.md`).
//!
//! A [`GrammarCore`] carries the phase it is in as a type parameter. The
//! textual parser builds a [`Draft`], whose sets, rules and tests refer to sets
//! by content hash. The `.cg3b` reader builds a [`Numbered`] grammar, whose
//! references are set numbers but which has none of the indexes a run reads.
//! `finish` consumes either and returns an [`Indexed`] grammar, the default
//! phase, and the only one that is written or run. Each phase has the
//! operations its references allow, so the order of the load is kept by the
//! compiler rather than by every caller.
//!
//! The phases are empty types: a phase change moves the grammar's fields into
//! the next phase's type and costs nothing.

use std::marker::PhantomData;

use super::GrammarCore;

mod sealed {
    pub trait Sealed {}
}

// [spec:cg3:req:grammar-phases.loaders]
/// A phase of a grammar's load. Sealed: the three phases are the whole set.
pub trait Phase: sealed::Sealed + Send + Sync + 'static {}

/// A phase whose set references are numbers: [`Numbered`] and [`Indexed`].
/// The lookups by set number exist in these.
pub trait Numbering: Phase {}

/// A grammar as the textual parser, or a caller building one by hand, makes
/// it: sets, rules and tests refer to sets by content hash, and the lookups
/// by content hash exist only here.
#[derive(Debug)]
pub enum Draft {}

/// A grammar as the `.cg3b` reader makes it, or as an indexed grammar gives
/// its indexes up to be edited: references are set numbers, and none of the
/// indexes a run reads are built.
#[derive(Debug)]
pub enum Numbered {}

/// A finished grammar: numbered and indexed, ready to be written or run.
#[derive(Debug)]
pub enum Indexed {}

impl sealed::Sealed for Draft {}
impl sealed::Sealed for Numbered {}
impl sealed::Sealed for Indexed {}
impl Phase for Draft {}
impl Phase for Numbered {}
impl Phase for Indexed {}
impl Numbering for Numbered {}
impl Numbering for Indexed {}

/// A grammar the textual parser is building.
pub type GrammarDraft = GrammarCore<Draft>;

/// A grammar read from a `.cg3b`, or given up by an indexed one, not yet
/// indexed.
pub type GrammarNumbered = GrammarCore<Numbered>;

impl<P: Phase> GrammarCore<P> {
    /// The same grammar, as phase `Q`. Every caller is a phase change that has
    /// just done what makes the grammar a `Q`.
    pub(super) fn into_phase<Q: Phase>(self) -> GrammarCore<Q> {
        let GrammarCore {
            rand_state,
            has_dep,
            has_bag_of_tags,
            has_relations,
            has_encl_final,
            has_protect,
            is_binary,
            sub_readings_ltr,
            ordered,
            addcohort_attach,
            grammar_size,
            num_tags,
            mapping_prefix,
            lines,
            verbosity_level,
            total_time,
            cmdargs,
            cmdargs_override,
            source_names,
            binary_path,
            single_tags_list,
            tags_by_hash,
            sets_list,
            sets_list_order,
            sets_all,
            sets_by_name,
            set_name_seeds,
            sets_by_contents,
            set_alias,
            maybe_used_sets,
            static_sets,
            regex_tags,
            icase_tags,
            contexts_arena,
            templates,
            contexts,
            rules_by_set,
            rules_by_tag,
            sets_by_tag,
            rules_any,
            sets_any,
            delimiters,
            soft_delimiters,
            text_delimiters,
            tag_any,
            preferred_targets,
            reopen_mappings,
            parentheses,
            parentheses_reverse,
            sections,
            anchors,
            rule_by_number,
            before_sections,
            rules,
            after_sections,
            null_section,
            wf_rules,
            phase: _,
        } = self;
        GrammarCore {
            rand_state,
            has_dep,
            has_bag_of_tags,
            has_relations,
            has_encl_final,
            has_protect,
            is_binary,
            sub_readings_ltr,
            ordered,
            addcohort_attach,
            grammar_size,
            num_tags,
            mapping_prefix,
            lines,
            verbosity_level,
            total_time,
            cmdargs,
            cmdargs_override,
            source_names,
            binary_path,
            single_tags_list,
            tags_by_hash,
            sets_list,
            sets_list_order,
            sets_all,
            sets_by_name,
            set_name_seeds,
            sets_by_contents,
            set_alias,
            maybe_used_sets,
            static_sets,
            regex_tags,
            icase_tags,
            contexts_arena,
            templates,
            contexts,
            rules_by_set,
            rules_by_tag,
            sets_by_tag,
            rules_any,
            sets_any,
            delimiters,
            soft_delimiters,
            text_delimiters,
            tag_any,
            preferred_targets,
            reopen_mappings,
            parentheses,
            parentheses_reverse,
            sections,
            anchors,
            rule_by_number,
            before_sections,
            rules,
            after_sections,
            null_section,
            wf_rules,
            phase: PhantomData,
        }
    }
}
