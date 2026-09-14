use std::collections::{BTreeMap, HashMap};

use crate::{
    def_ids::DefId,
    mir::{
        self, BasicBlockId, Operand, Place, PlaceProjection, Rvalue, SwitchTarget, SwitchTargets,
        TerminatorKind, Value, build::Builder,
    },
    src_loc::SrcLoc,
    typed_ast::{CaseArm, Expr, FieldId, Pattern, PatternKind},
    types::{CaseId, Type},
};
enum Test {
    VariantSwitch(DefId),
    IntSwitch,
    If,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum TestCase {
    True,
    False,
    EqualsChar(char),
    EqualsInt(i128),
    Variant(DefId, CaseId),
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extraction {
    Field(FieldId),
    Payload(CaseId),
}
type TestMatrix<'ctxt> = Vec<(SrcLoc, usize, Vec<MatchTest<'ctxt>>)>;
#[derive(Debug, Clone)]
struct MatchTest<'ctxt> {
    extractions: Vec<Extraction>,
    value: Value<'ctxt>,
    case: TestCase,
    loc: SrcLoc,
}
impl<'ctxt> Builder<'_, 'ctxt> {
    fn build_tree(
        &mut self,
        tests: TestMatrix<'ctxt>,
        end_blocks: &mut Vec<(SrcLoc, usize, BasicBlockId)>,
    ) -> BasicBlockId {
        /* No more arms */
        let Some(&(loc, index, ref row)) = tests.first() else {
            return self.current_block;
        };
        let Some(head_test) = row.first() else {
            end_blocks.push((loc, index, self.current_block));
            return self.current_block;
        };
        let head_test = head_test.clone();
        let test = match head_test.case {
            TestCase::EqualsInt(_) => Test::IntSwitch,
            TestCase::False | TestCase::True => Test::If,
            TestCase::Variant(def_id, _) => Test::VariantSwitch(def_id),
            TestCase::EqualsChar(_) => Test::IntSwitch,
        };
        fn group_tests<'ctxt>(
            value: &Value<'ctxt>,
            projections: &[Extraction],
            tests: TestMatrix<'ctxt>,
        ) -> (BTreeMap<TestCase, TestMatrix<'ctxt>>, TestMatrix<'ctxt>) {
            let mut branches: BTreeMap<TestCase, TestMatrix> = BTreeMap::new();
            let mut others = TestMatrix::new();
            for mut row in tests {
                let Some(head) = row.2.first() else {
                    others.push(row);
                    continue;
                };
                let &MatchTest {
                    value: ref head_value,
                    extractions: ref head_extractions,
                    case,
                    loc: _,
                } = head;
                if value != head_value && head_extractions != projections {
                    others.push(row);
                    continue;
                }
                row.2.remove(0);
                branches.entry(case).or_default().push(row);
            }
            (branches, others)
        }
        let (tests, rest) = group_tests(&head_test.value, &head_test.extractions, tests);

        let start_block = self.current_block;
        let otherwise_start = self.switch_to_new_block();
        let tests = tests
            .into_iter()
            .map(|(case, info)| {
                let start_branch = self.switch_to_new_block();
                let otherwise_block = self.build_tree(info, end_blocks);
                self.switch_to_block(otherwise_block);
                self.finish_block_with_goto(head_test.loc, otherwise_start);
                (case, start_branch)
            })
            .collect::<HashMap<_, _>>();

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

                let mut value = head_test.value;
                for extraction in head_test.extractions {
                    value = Value::Reg(self.push_operation(
                        head_test.loc,
                        match extraction {
                            Extraction::Field(field) => mir::Operation::ExtractField(value, field),
                            Extraction::Payload(case_id) => {
                                mir::Operation::ExtractPayload(value, case_id)
                            }
                        },
                    ));
                }
                self.finish_block_with_if(head_test.loc, value, true_block, false_block);
            }
            Test::IntSwitch => {
                let targets = tests
                    .into_iter()
                    .filter_map(|(case, block)| {
                        let value = match case {
                            TestCase::EqualsInt(value) => value,
                            TestCase::EqualsChar(char) => u32::from(char).into(),
                            _ => return None,
                        };
                        Some(SwitchTarget {
                            value,
                            target: block,
                        })
                    })
                    .collect();
                let mut value = head_test.value;
                for extraction in head_test.extractions {
                    value = Value::Reg(self.push_operation(
                        head_test.loc,
                        match extraction {
                            Extraction::Field(field) => mir::Operation::ExtractField(value, field),
                            Extraction::Payload(case_id) => {
                                mir::Operation::ExtractPayload(value, case_id)
                            }
                        },
                    ));
                }
                self.finish_block_with_switch(
                    head_test.loc,
                    value,
                    SwitchTargets {
                        targets,
                        otherwise: otherwise_start,
                    },
                );
            }
            Test::VariantSwitch(id) => {
                let type_def = self.ctxt.type_def(id);

                let targets = tests
                    .iter()
                    .filter_map(|(case, block)| {
                        let TestCase::Variant(_, id) = *case else {
                            return None;
                        };
                        Some(SwitchTarget {
                            value: type_def.case_value(id).1.into(),
                            target: *block,
                        })
                    })
                    .collect();

                self.switch_to_block(start_block);
                let mut value = head_test.value;
                for extraction in head_test.extractions {
                    value = Value::Reg(self.push_operation(
                        head_test.loc,
                        match extraction {
                            Extraction::Field(field) => mir::Operation::ExtractField(value, field),
                            Extraction::Payload(case_id) => {
                                mir::Operation::ExtractPayload(value, case_id)
                            }
                        },
                    ));
                }
                let discriminant = Value::Reg(
                    self.push_operation(head_test.loc, mir::Operation::Discriminant(value)),
                );
                self.finish_block_with_switch(
                    head_test.loc,
                    discriminant,
                    SwitchTargets {
                        targets,
                        otherwise: otherwise_start,
                    },
                );
            }
        }
        self.switch_to_block(otherwise_start);
        self.build_tree(rest, end_blocks)
    }
    fn match_tests(
        &self,
        value: Value<'ctxt>,
        extractions: Vec<Extraction>,
        pattern: &Pattern<'ctxt>,
    ) -> Vec<MatchTest<'ctxt>> {
        match &pattern.kind {
            PatternKind::Char(c) => {
                vec![MatchTest {
                    extractions,
                    value,
                    case: TestCase::EqualsChar(*c),
                    loc: pattern.loc,
                }]
            }
            PatternKind::Case(def_id, _, index, inner) => {
                let def_id = &self.ctxt.expect_parent(*def_id);
                if let Some(inner) = inner {
                    let mut tests = vec![MatchTest {
                        extractions: extractions.clone(),
                        value: value.clone(),
                        case: TestCase::Variant(*def_id, *index),
                        loc: pattern.loc,
                    }];
                    tests.extend(self.match_tests(
                        value,
                        {
                            let mut extractions = extractions;
                            extractions.push(Extraction::Payload(*index));
                            extractions.push(Extraction::Field(FieldId::new(0)));
                            extractions
                        },
                        inner,
                    ));
                    tests
                } else {
                    vec![MatchTest {
                        extractions,
                        value,
                        case: TestCase::Variant(*def_id, *index),
                        loc: pattern.loc,
                    }]
                }
            }
            PatternKind::Int(numeric_value) => {
                vec![MatchTest {
                    extractions,
                    value,
                    case: TestCase::EqualsInt(*numeric_value as i128),
                    loc: pattern.loc,
                }]
            }
            PatternKind::Bool(bool_value) => vec![MatchTest {
                loc: pattern.loc,
                extractions,
                value,
                case: if *bool_value {
                    TestCase::True
                } else {
                    TestCase::False
                },
            }],
            PatternKind::Binding(..) | PatternKind::Err | PatternKind::Unit => Vec::new(),
            PatternKind::Record(pattern_fields) => pattern_fields
                .iter()
                .flat_map(|field| {
                    self.match_tests(
                        value.clone(),
                        {
                            let mut extractions = extractions.clone();
                            extractions.push(Extraction::Field(field.index));
                            extractions
                        },
                        &field.pattern,
                    )
                })
                .collect(),
        }
    }

    pub(super) fn build_match(
        &mut self,
        result_ty: Type<'ctxt>,
        expr: &Expr<'ctxt>,
        arms: &[CaseArm<'ctxt>],
    ) -> Value<'ctxt> {
        let value = self.expr_value(expr);
        let tests = arms
            .iter()
            .enumerate()
            .map(|(i, arm)| {
                (
                    arm.pattern.loc,
                    i,
                    self.match_tests(value.clone(), Vec::new(), &arm.pattern),
                )
            })
            .collect::<Vec<_>>();
        let mut end_blocks = Vec::new();
        self.build_tree(tests, &mut end_blocks);
        self.finish_block(expr.loc, TerminatorKind::Unreachable);

        let (end_block, [result]) = self.new_block_with_args([result_ty]);
        for (loc, i, block) in end_blocks.into_iter() {
            self.switch_to_block(block);
            self.assign_to_pattern(arms[i].pattern.loc, &arms[i].pattern, value.clone());
            let result = self.expr_value(&arms[i].body);
            self.finish_block_with_goto_args(loc, end_block, [result]);
        }
        self.switch_to_block(end_block);
        Value::Reg(result)
    }
}
