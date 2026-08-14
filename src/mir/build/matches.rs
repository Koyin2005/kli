use std::collections::{BTreeMap, HashMap};

use crate::{
    mir::{BasicBlockId, Operand, Place, Rvalue, SwitchTarget, TerminatorKind, build::Builder},
    src_loc::SrcLoc,
    typed_ast::{CaseArm, Expr, FieldId, Pattern, PatternKind},
    types::{CaseId, IntegerSize, Type},
};
enum Test {
    VariantSwitch,
    IntSwitch,
    If,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum TestCase {
    True,
    False,
    Equals(i128),
    Variant(CaseId),
}
type TestMatrix = Vec<(SrcLoc, Vec<MatchTest>)>;
#[derive(Debug, Clone)]
struct MatchTest {
    place: Place,
    case: TestCase,
    loc: SrcLoc,
}
impl<'ctxt> Builder<'_, 'ctxt> {
    fn build_tree(
        &mut self,
        tests: TestMatrix,
        end_blocks: &mut Vec<(SrcLoc, BasicBlockId)>,
    ) -> BasicBlockId {
        /* No more arms */
        let Some(&(loc, ref row)) = tests.first() else {
            return self.current_block;
        };
        let Some(head_test) = row.first() else {
            end_blocks.push((loc, self.current_block));
            return self.current_block;
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
                    loc: _,
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
        let (tests, rest) = group_tests(&head_test.place, tests);

        let start_block = self.current_block;

        let mut otherwise_blocks = Vec::with_capacity(tests.len());
        let tests = tests
            .into_iter()
            .map(|(case, info)| {
                let start_branch = self.switch_to_new_block();
                let otherwise_block = self.build_tree(info, end_blocks);
                otherwise_blocks.push(otherwise_block);
                (case, start_branch)
            })
            .collect::<HashMap<_, _>>();

        let otherwise_start = self.switch_to_new_block();
        for block in otherwise_blocks {
            self.switch_to_block(block);
            self.finish_block_with_goto(head_test.loc, otherwise_start);
        }
        self.switch_to_block(start_block);
        match test {
            Test::If => {
                let true_block = tests
                    .get(&TestCase::True)
                    .copied()
                    .unwrap_or(otherwise_start);
                let false_block = tests
                    .get(&TestCase::False)
                    .copied()
                    .unwrap_or(otherwise_start);
                self.finish_block_with_if(
                    head_test.loc,
                    Operand::Load(head_test.place),
                    true_block,
                    false_block,
                );
            }
            Test::IntSwitch => {
                let targets = tests
                    .into_iter()
                    .filter_map(|(case, block)| {
                        let TestCase::Equals(value) = case else {
                            return None;
                        };
                        Some(SwitchTarget {
                            value,
                            target: block,
                        })
                    })
                    .collect();
                self.finish_block_with_switch_targets(
                    head_test.loc,
                    Operand::Load(head_test.place),
                    targets,
                    otherwise_start,
                );
            }
            Test::VariantSwitch => {
                let (id, _, _) = head_test
                    .place
                    .type_of(self.ctxt, &self.body.locals, self.body.return_type)
                    .as_named()
                    .unwrap();
                let type_def = self.ctxt.type_def(id);
                let targets = tests
                    .iter()
                    .filter_map(|(case, block)| {
                        let TestCase::Variant(id) = *case else {
                            return None;
                        };
                        Some(SwitchTarget {
                            value: type_def.case_value(id).1 as i128,
                            target: *block,
                        })
                    })
                    .collect();
                self.switch_to_block(start_block);
                let disrciminant = self.assign_to_temp(
                    head_test.loc,
                    Type::new_uint(self.ctxt, IntegerSize::Int64),
                    Rvalue::Discriminant(head_test.place),
                );
                self.finish_block_with_switch_targets(
                    head_test.loc,
                    Operand::Load(Place::local(disrciminant)),
                    targets,
                    otherwise_start,
                );
            }
        }
        self.switch_to_block(otherwise_start);
        self.build_tree(rest, end_blocks)
    }
    fn match_tests(&self, place: Place, pattern: &Pattern) -> Vec<MatchTest> {
        match &pattern.kind {
            PatternKind::Case(id, .., index, inner) => {
                if let Some(inner) = inner {
                    let mut tests = vec![MatchTest {
                        place: place.clone(),
                        case: TestCase::Variant(*index),
                        loc: pattern.loc,
                    }];
                    tests.extend(
                        self.match_tests(
                            place
                                .with_case_downcast(*index, self.ctxt.expect_ident(*id).symbol)
                                .with_field(FieldId::new(0)),
                            inner,
                        ),
                    );
                    tests
                } else {
                    vec![MatchTest {
                        place,
                        case: TestCase::Variant(*index),
                        loc: pattern.loc,
                    }]
                }
            }
            PatternKind::Int(value) => {
                vec![MatchTest {
                    place,
                    case: TestCase::Equals(*value as i128),
                    loc: pattern.loc,
                }]
            }
            PatternKind::Bool(value) => vec![MatchTest {
                loc: pattern.loc,
                place,
                case: if *value {
                    TestCase::True
                } else {
                    TestCase::False
                },
            }],
            PatternKind::Binding(..) | PatternKind::Err | PatternKind::Unit => Vec::new(),
            PatternKind::Record(pattern_fields) => pattern_fields
                .iter()
                .flat_map(|field| {
                    self.match_tests(place.clone().with_field(field.index), &field.pattern)
                })
                .collect(),
        }
    }

    pub(super) fn build_match(&mut self, dest: Place, expr: &Expr<'ctxt>, arms: &[CaseArm<'ctxt>]) {
        let place = self.place(expr);
        let tests = arms
            .iter()
            .map(|arm| {
                (
                    arm.pattern.loc,
                    self.match_tests(place.clone(), &arm.pattern),
                )
            })
            .collect::<Vec<_>>();
        let mut end_blocks = Vec::new();
        self.build_tree(tests, &mut end_blocks);
        self.finish_block(expr.loc, TerminatorKind::Unreachable);

        let end_block = self.switch_to_new_block();
        for (i, (loc, block)) in end_blocks.into_iter().enumerate() {
            self.switch_to_block(block);
            self.assign_place_to_pattern(&arms[i].pattern, place.clone());
            self.expr_into_dest(dest.clone(), &arms[i].body);
            self.finish_block_with_goto(loc, end_block);
        }
        self.switch_to_block(end_block);
    }
}
