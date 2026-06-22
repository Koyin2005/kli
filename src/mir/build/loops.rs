use crate::{
    mir::{
        self, BinaryOp, Constant, Operand, OverflowOp, Place, Rvalue, SwitchTarget, SwitchTargets,
        build::Builder,
    },
    typed_ast::{Expr, FieldId, IteratorType, Pattern},
    types::{self, LIST_LEN_FIELD, LIST_PTR_FIELD, Type},
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
            IteratorType::ArrayListRef(region, mutable, ty) => {
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
                let len = self.len_operand(
                    iterator.ty.as_reference_type().unwrap().2,
                    place.clone().with_deref(),
                );
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
                let current_element = {
                    let place = place.with_deref().with_field(types::LIST_PTR_FIELD);
                    let offset_ptr = self.assign_to_temp(
                        Type::pointer(ty.clone()),
                        Rvalue::Binary(
                            BinaryOp::Offset,
                            Box::new((
                                Operand::Load(place.clone()),
                                Operand::Load(Place::local(current_index)),
                            )),
                        ),
                    );
                    Place::local(self.assign_to_temp(
                        Type::reference(ty.clone(), *mutable, region.clone()),
                        Rvalue::PointerCast(
                            mir::PointerCast::RawToRef(*mutable),
                            Operand::Load(Place::local(offset_ptr)),
                        ),
                    ))
                };
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
                /*
                 for c in &s{
                   ..body
                 }
                 header:
                 byte_ptr = s^.byte_ptr;
                 i = 0;
                 loop:
                 if i < s^.len goto body else goto end
                 body
                   res = decode_utf8(byte_ptr,i);
                   c = res.c;
                   i = res.i;
                   pat(c)
                   ..body
                   goto loop
                 end

                */
                let string_ref = self.place(iterator);
                let byte_ptr = string_ref.clone().with_deref().with_field(LIST_PTR_FIELD);
                let current_index = self
                    .assign_to_temp(Type::Int, Rvalue::Use(Operand::Constant(Constant::int(0))));
                self.goto_to_new_block();

                let in_bounds = self.assign_to_temp(
                    Type::Bool,
                    Rvalue::Binary(
                        BinaryOp::Lesser,
                        Box::new((
                            Operand::Load(Place::local(current_index)),
                            Operand::Load(string_ref.with_deref().with_field(LIST_LEN_FIELD)),
                        )),
                    ),
                );

                let loop_block = self.current_block;
                self.switch_to_new_block();

                let body_block = self.current_block;
                let result = self.assign_to_temp(
                    Type::record([Type::Char, Type::Int].into()),
                    Rvalue::DecodeUtf8(
                        Operand::Load(byte_ptr),
                        Operand::Load(Place::local(current_index)),
                    ),
                );
                self.assign_place_to_pattern(
                    pattern,
                    Place::local(result).with_field(FieldId::zero()),
                );
                self.expr_stmt(body);
                self.assign(
                    Place::local(current_index),
                    Rvalue::Use(Operand::Load(
                        Place::local(result).with_field(FieldId::new(1)),
                    )),
                );
                self.finish_block_with_goto(loop_block);

                let end_block = self.current_block;
                self.switch_to_block(loop_block);
                self.finish_block_with_switch(
                    Operand::Load(Place::local(in_bounds)),
                    SwitchTargets {
                        targets: vec![SwitchTarget {
                            value: 1,
                            target: body_block,
                        }],
                        otherwise: end_block,
                    },
                );
                self.switch_to_block(end_block);
            }
        }
    }
}
