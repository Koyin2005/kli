use std::collections::BTreeMap;

use crate::{
    mir::{BasicBlockId, Operand, Place, Rvalue, SwitchTarget, TerminatorKind, build::Builder},
    src_loc::SrcLoc,
    typed_ast::{CaseArm, Expr, Pattern, PatternKind},
    types::Type,
};

struct MatchInfo<'a> {
    dest: &'a Place,
    pattern_place: &'a Place,
    arms: &'a [CaseArm],
}
enum Test {
    VariantSwitch,
    IntSwitch,
    If,
}
#[derive(Debug, Clone)]
enum MatchBranch {
    IntSwitch(Place, Vec<(i64, MatchBranch)>, Box<MatchBranch>),
    VariantSwitch(Place, Vec<(usize, MatchBranch)>, Box<MatchBranch>),
    If {
        place: Place,
        true_tree: Box<MatchBranch>,
        false_tree: Box<MatchBranch>,
    },
    Success(usize),
    Unreachable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum TestCase {
    True,
    False,
    Equals(i64),
    Variant(usize),
}
type TestMatrix = Vec<(usize, Vec<MatchTest>)>;
#[derive(Debug, Clone)]
struct MatchTest {
    place: Place,
    case: TestCase,
}
impl Builder<'_> {
    fn build_tree(&mut self, tests: TestMatrix) -> MatchBranch {
        let Some(head_row) = tests.first() else {
            return MatchBranch::Unreachable;
        };
        let Some(head_test) = head_row.1.first() else {
            return MatchBranch::Success(head_row.0);
        };
        let head_test = head_test.clone();
        let test = match head_test.case {
            TestCase::Equals(_) => Test::IntSwitch,
            TestCase::False | TestCase::True => Test::If,
            TestCase::Variant(_) => Test::VariantSwitch,
        };
        fn group_tests(
            place: &Place,
            tests: TestMatrix,
        ) -> (BTreeMap<TestCase, TestMatrix>, TestMatrix) {
            let mut branches: BTreeMap<TestCase, TestMatrix> = BTreeMap::new();
            let mut others = TestMatrix::new();
            for mut row in tests {
                let Some(head) = row.1.first() else {
                    others.push(row);
                    continue;
                };
                let &MatchTest {
                    place: ref head_place,
                    case,
                } = head;
                if head_place != place {
                    others.push(row);
                    continue;
                }
                row.1.remove(0);
                branches.entry(case).or_default().push(row);
            }
            (branches, others)
        }
        let (mut tests, rest) = group_tests(&head_test.place, tests);
        let mut build_tree =
            |this: &mut Builder, case| tests.remove(case).map(|tests| this.build_tree(tests));
        match test {
            Test::If => {
                let otherwise_branch = self.build_tree(rest);
                let true_branch =
                    build_tree(self, &TestCase::True).unwrap_or_else(|| otherwise_branch.clone());
                let false_branch = build_tree(self, &TestCase::False).unwrap_or(otherwise_branch);
                MatchBranch::If {
                    place: head_test.place,
                    true_tree: Box::new(true_branch),
                    false_tree: Box::new(false_branch),
                }
            }
            Test::IntSwitch => {
                let cases = tests
                    .into_iter()
                    .map(|(case, row)| {
                        let TestCase::Equals(value) = case else {
                            unreachable!("should only be ints")
                        };
                        (value, self.build_tree(row))
                    })
                    .collect::<Vec<_>>();
                MatchBranch::IntSwitch(head_test.place, cases, Box::new(self.build_tree(rest)))
            }
            Test::VariantSwitch => {
                let cases = tests
                    .into_iter()
                    .map(|(case, row)| {
                        let TestCase::Variant(index) = case else {
                            unreachable!("should only be ints")
                        };
                        (index, self.build_tree(row))
                    })
                    .collect::<Vec<_>>();
                MatchBranch::VariantSwitch(head_test.place, cases, Box::new(self.build_tree(rest)))
            }
        }
    }
    fn match_tests(&self, place: Place, pattern: &Pattern) -> Vec<MatchTest> {
        match &pattern.kind {
            PatternKind::Case(id, .., index, inner) => {
                if let Some(inner) = inner {
                    let mut tests = vec![MatchTest {
                        place: place.clone(),
                        case: TestCase::Variant(*index),
                    }];
                    tests.extend(self.match_tests(
                        place.with_case_downcast(*index, self.ctxt.name(*id).symbol),
                        inner,
                    ));
                    tests
                } else {
                    vec![MatchTest {
                        place,
                        case: TestCase::Variant(*index),
                    }]
                }
            }
            PatternKind::Int(value) => {
                vec![MatchTest {
                    place,
                    case: TestCase::Equals(*value),
                }]
            }
            PatternKind::Bool(value) => vec![MatchTest {
                place,
                case: if *value {
                    TestCase::True
                } else {
                    TestCase::False
                },
            }],
            PatternKind::Ref(pattern) => self.match_tests(place.with_deref(), pattern),
            PatternKind::Binding(..) | PatternKind::Err => Vec::new(),
            PatternKind::Record(pattern_fields) => pattern_fields
                .iter()
                .flat_map(|field| {
                    self.match_tests(place.clone().with_field(field.index), &field.pattern)
                })
                .collect(),
        }
    }
    fn lower_tree(
        &mut self,
        loc: SrcLoc,
        tree: MatchBranch,
        info: &'_ MatchInfo,
        end_blocks: &mut Vec<(SrcLoc, BasicBlockId)>,
    ) {
        let start_block = self.current_block;
        match tree {
            MatchBranch::IntSwitch(place, arms, otherwise_branch) => {
                let targets = arms
                    .into_iter()
                    .map(|(value, arm)| {
                        let block = self.switch_to_new_block();
                        self.lower_tree(loc, arm, info, end_blocks);
                        SwitchTarget {
                            value: value.into(),
                            target: block,
                        }
                    })
                    .collect();
                let otherwise = self.switch_to_new_block();
                self.lower_tree(loc, *otherwise_branch, info, end_blocks);

                self.switch_to_block(start_block);
                self.finish_block_with_switch_targets(
                    loc,
                    Operand::Load(place),
                    targets,
                    otherwise,
                );
            }
            MatchBranch::VariantSwitch(place, arms, otherwise_branch) => {
                let targets = arms
                    .into_iter()
                    .map(|(value, arm)| {
                        let block = self.switch_to_new_block();
                        self.lower_tree(loc, arm, info, end_blocks);
                        SwitchTarget {
                            value: value.try_into().unwrap(),
                            target: block,
                        }
                    })
                    .collect();
                let otherwise = self.switch_to_new_block();
                self.lower_tree(loc, *otherwise_branch, info, end_blocks);

                self.switch_to_block(start_block);
                let disrciminant = self.assign_to_temp(loc, Type::Int, Rvalue::Discriminant(place));
                self.finish_block_with_switch_targets(
                    loc,
                    Operand::Load(Place::local(disrciminant)),
                    targets,
                    otherwise,
                );
            }
            MatchBranch::If {
                place,
                true_tree,
                false_tree,
            } => {
                let true_block = self.switch_to_new_block();
                self.lower_tree(loc, *true_tree, info, end_blocks);

                let false_block = self.switch_to_new_block();
                self.lower_tree(loc, *false_tree, info, end_blocks);

                self.switch_to_block(start_block);
                self.finish_block_with_if(loc, Operand::Load(place), true_block, false_block);
            }
            MatchBranch::Success(i) => {
                self.assign_place_to_pattern(&info.arms[i].pattern, info.pattern_place.clone());
                self.expr_into_dest(info.dest.clone(), &info.arms[i].body);
                end_blocks.push((info.arms[i].body.loc, self.current_block));
            }
            MatchBranch::Unreachable => {
                self.finish_block(loc, TerminatorKind::Unreachable);
            }
        }
    }
    pub(super) fn build_match(&mut self, dest: Place, expr: &Expr, arms: &[CaseArm]) {
        let place = self.place(expr);
        let tests = arms
            .iter()
            .enumerate()
            .map(|(i, arm)| (i, self.match_tests(place.clone(), &arm.pattern)))
            .collect::<Vec<_>>();
        let tree = self.build_tree(tests);
        let mut end_blocks = Vec::new();
        self.lower_tree(
            expr.loc,
            tree,
            &MatchInfo {
                dest: &dest,
                pattern_place: &place,
                arms,
            },
            &mut end_blocks,
        );
        let end_block = self.switch_to_new_block();
        for (loc, block) in end_blocks {
            self.switch_to_block(block);
            self.finish_block_with_goto(loc, end_block);
        }
        self.switch_to_block(end_block);
    }
}
