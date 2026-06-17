use crate::{
    mir::{
        self, BinaryOp, Constant, Operand, OverflowOp, Place, Rvalue, SwitchTarget, SwitchTargets,
        build::Builder,
    },
    typed_ast::{Expr, IteratorType, Pattern},
    types::Type,
};

impl Builder<'_> {
    pub(super) fn for_loop(
        &mut self,
        pattern: &Pattern,
        iterator: &Expr,
        iterator_type: &IteratorType,
        body: &Expr,
    ) {
        match iterator_type {
            IteratorType::ArrayListRef(..) => {
                /*
                   for i in &l{
                       stuff
                   }

                   bb_header
                    iter = &l
                    i = 0
                    goto bb_cond
                   bb_cond
                    in_bounds = i < len(iter^)
                    switch in_bounds 0 -> bb_end, otherwise -> bb_body
                   bb_body
                    ....
                    i = i + 1;
                    goto bb_cond
                   bb_end
                */
                let place = self.place(iterator);
                let current_index = self
                    .assign_to_temp(Type::Int, Rvalue::Use(Operand::Constant(Constant::int(0))));
                self.goto_to_new_block();

                //Condition
                let len = self.len_operand(place.clone().with_deref());
                let in_bounds = self.assign_to_temp(
                    Type::Bool,
                    Rvalue::Binary(
                        BinaryOp::Lesser,
                        Box::new((Operand::Load(Place::local(current_index)), len.clone())),
                    ),
                );
                let cond_block = self.current_block;

                //Body
                let loop_body_start_block = self.new_block();
                self.switch_to_block(loop_body_start_block);
                let current_element = place.with_deref().with_index(current_index);
                self.assign_place_to_pattern(pattern, current_element);
                self.expr_stmt(body);
                self.assign(
                    Place::local(current_index),
                    Rvalue::Binary(
                        mir::BinaryOp::Unchecked(OverflowOp::Add),
                        Box::new((
                            Operand::Load(Place::local(current_index)),
                            Operand::Constant(Constant::int(1)),
                        )),
                    ),
                );
                self.finish_block_with_goto(cond_block);
                self.switch_to_new_block();
                let end_block = self.current_block;
                self.switch_to_block(cond_block);
                self.finish_block_with_switch(
                    Operand::Load(Place::local(in_bounds)),
                    SwitchTargets {
                        targets: vec![SwitchTarget {
                            value: 0,
                            target: end_block,
                        }],
                        otherwise: loop_body_start_block,
                    },
                );
                self.switch_to_block(end_block);
            }
            IteratorType::StringIter(..) => {
                todo!("Char iterator")
            }
        }
    }
}
