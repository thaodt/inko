//! Virtual Machine for running instructions
//!
//! A VirtualMachine manages threads, runs instructions, starts/terminates
//! threads and so on. VirtualMachine instances are fully self contained
//! allowing multiple instances to run fully isolated in the same process.

use std::io::{self, Write};
use std::thread;
use std::sync::{Arc, RwLock};
use std::sync::mpsc::channel;

use call_frame::CallFrame;
use compiled_code::RcCompiledCode;
use instruction::{InstructionType, Instruction};
use memory_manager::{MemoryManager, RcMemoryManager};
use object::RcObject;
use object_value;
use virtual_machine_methods::VirtualMachineMethods;
use thread::{Thread, RcThread};
use thread_list::ThreadList;

/// A reference counted VirtualMachine.
pub type RcVirtualMachine = Arc<VirtualMachine>;

/// Structure representing a single VM instance.
pub struct VirtualMachine {
    // All threads that are currently active.
    threads: RwLock<ThreadList>,

    // The struct for allocating/managing memory.
    memory_manager: RcMemoryManager,

    // The status of the VM when exiting.
    exit_status: RwLock<Result<(), ()>>
}

impl VirtualMachine {
    pub fn new() -> RcVirtualMachine {
        let vm = VirtualMachine {
            threads: RwLock::new(ThreadList::new()),
            memory_manager: MemoryManager::new(),
            exit_status: RwLock::new(Ok(()))
        };

        Arc::new(vm)
    }
}

impl VirtualMachineMethods for RcVirtualMachine {
    fn start(&self, code: RcCompiledCode) -> Result<(), ()> {
        let thread_obj = self.run_thread(code, true);
        let vm_thread  = write_lock!(thread_obj).value.as_thread();
        let handle     = vm_thread.take_join_handle();

        if handle.is_some() {
            handle.unwrap().join().unwrap();
        }

        *read_lock!(self.exit_status)
    }

    fn run(&self, thread: RcThread,
               code: RcCompiledCode) -> Result<Option<RcObject>, String> {
        if thread.should_stop() {
            return Ok(None);
        }

        let mut skip_until: Option<usize> = None;
        let mut retval = None;

        let mut index = 0;
        let count = code.instructions.len();

        while index < count {
            let ref instruction = code.instructions[index];

            if skip_until.is_some() {
                if index < skip_until.unwrap() {
                    continue;
                }
                else {
                    skip_until = None;
                }
            }

            // Incremented _before_ the instructions so that the "goto"
            // instruction can overwrite it.
            index += 1;

            match instruction.instruction_type {
                InstructionType::SetInteger => {
                    try!(self.ins_set_integer(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetFloat => {
                    try!(self.ins_set_float(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetString => {
                    try!(self.ins_set_string(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetObject => {
                    try!(self.ins_set_object(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetArray => {
                    try!(self.ins_set_array(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetName => {
                    try!(self.ins_set_name(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetIntegerPrototype => {
                    try!(self.ins_set_integer_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetFloatPrototype => {
                    try!(self.ins_set_float_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetStringPrototype => {
                    try!(self.ins_set_string_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetArrayPrototype => {
                    try!(self.ins_set_array_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetThreadPrototype => {
                    try!(self.ins_set_thread_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetTruePrototype => {
                    try!(self.ins_set_true_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetFalsePrototype => {
                    try!(self.ins_set_false_prototype(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetTrue => {
                    try!(self.ins_set_true(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetFalse => {
                    try!(self.ins_set_false(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetLocal => {
                    try!(self.ins_set_local(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::GetLocal => {
                    try!(self.ins_get_local(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetConst => {
                    try!(self.ins_set_const(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::GetConst => {
                    try!(self.ins_get_const(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::SetAttr => {
                    try!(self.ins_set_attr(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::GetAttr => {
                    try!(self.ins_get_attr(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::Send => {
                    try!(self.ins_send(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::Return => {
                    retval = try!(self.ins_return(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::GotoIfFalse => {
                    skip_until = try!(self.ins_goto_if_false(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::GotoIfTrue => {
                    skip_until = try!(self.ins_goto_if_true(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::Goto => {
                    index = try!(self.ins_goto(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::DefMethod => {
                    try!(self.ins_def_method(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::RunCode => {
                    try!(self.ins_run_code(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::GetToplevel => {
                    try!(self.ins_get_toplevel(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerAdd => {
                    try!(self.ins_integer_add(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerDiv => {
                    try!(self.ins_integer_div(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerMul => {
                    try!(self.ins_integer_mul(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerSub => {
                    try!(self.ins_integer_sub(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerMod => {
                    try!(self.ins_integer_mod(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerToFloat => {
                    try!(self.ins_integer_to_float(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerToString => {
                    try!(self.ins_integer_to_string(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerBitwiseAnd => {
                    try!(self.ins_integer_bitwise_and(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerBitwiseOr => {
                    try!(self.ins_integer_bitwise_or(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerBitwiseXor => {
                    try!(self.ins_integer_bitwise_xor(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerShiftLeft => {
                    try!(self.ins_integer_shift_left(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerShiftRight => {
                    try!(self.ins_integer_shift_right(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerSmaller => {
                    try!(self.ins_integer_smaller(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerGreater => {
                    try!(self.ins_integer_greater(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::IntegerEqual => {
                    try!(self.ins_integer_equal(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                },
                InstructionType::StartThread => {
                    try!(self.ins_start_thread(
                        thread.clone(),
                        code.clone(),
                        &instruction
                    ));
                }
            };
        }

        Ok(retval)
    }

    fn ins_set_integer(&self, thread: RcThread, code: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot".to_string())
        );

        let index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing integer literal index".to_string())
        );

        let value = *try!(
            code.integer_literals
                .get(index)
                .ok_or("undefined integer literal".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(value), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_set_float(&self, thread: RcThread, code: RcCompiledCode,
                     instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot".to_string())
        );

        let index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing float literal index".to_string())
        );

        let value  = *try!(
            code.float_literals
                .get(index)
                .ok_or("undefined float literal".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .float_prototype()
                .ok_or("no Float prototype set up".to_string())
        );

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::float(value), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_set_string(&self, thread: RcThread, code: RcCompiledCode,
                      instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index".to_string())
        );

        let index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing string literal index".to_string())
        );

        let value = try!(
            code.string_literals
                .get(index)
                .ok_or("undefined string literal".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .string_prototype()
                .ok_or("no String prototype set up".to_string())
        );

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::string(value.clone()), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_set_object(&self, thread: RcThread, _: RcCompiledCode,
                      instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index".to_string())
        );

        let proto_index_opt = instruction.arguments.get(1);

        let obj = write_lock!(self.memory_manager)
            .new_object(object_value::none());

        if proto_index_opt.is_some() {
            let proto_index = *proto_index_opt.unwrap();

            let proto = try!(
                thread.get_register(proto_index)
                    .ok_or("prototype is undefined".to_string())
            );

            write_lock!(obj).set_prototype(proto);
        }

        write_lock!(self.memory_manager)
            .allocate_prepared(obj.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_set_array(&self, thread: RcThread, _: RcCompiledCode,
                     instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index".to_string())
        );

        let val_count = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing value count".to_string())
        );

        let values = try!(
            self.collect_arguments(thread.clone(), instruction, 2, val_count)
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .array_prototype()
                .ok_or("no Array prototype set up".to_string())
        );

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::array(values), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_set_name(&self, thread: RcThread, code: RcCompiledCode,
                    instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing string literal index".to_string())
        );

        let obj = try!(
            thread.get_register(slot)
                .ok_or("undefined target object".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        write_lock!(obj).set_name(name.clone());

        Ok(())
    }

    fn ins_set_integer_prototype(&self, thread: RcThread, _: RcCompiledCode,
                                 instruction: &Instruction) -> Result<(), String> {
        if read_lock!(self.memory_manager).integer_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_integer_prototype(object);

        Ok(())
    }

    fn ins_set_float_prototype(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction) -> Result<(), String> {
        if read_lock!(self.memory_manager).float_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_float_prototype(object);

        Ok(())
    }

    fn ins_set_string_prototype(&self, thread: RcThread, _: RcCompiledCode,
                                instruction: &Instruction) -> Result<(), String> {
        if read_lock!(self.memory_manager).string_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_string_prototype(object);

        Ok(())
    }

    fn ins_set_array_prototype(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction)
                               -> Result<(), String> {
        if read_lock!(self.memory_manager).array_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_array_prototype(object);

        Ok(())
    }

    fn ins_set_thread_prototype(&self, thread: RcThread, _: RcCompiledCode,
                                instruction: &Instruction)
                                -> Result<(), String> {
        if read_lock!(self.memory_manager).thread_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_thread_prototype(object.clone());

        // Update the prototype of all existing threads (usually only the main
        // thread at this point).
        write_lock!(self.threads).set_prototype(object);

        Ok(())
    }

    fn ins_set_true_prototype(&self, thread: RcThread, _: RcCompiledCode,
                              instruction: &Instruction) -> Result<(), String> {
        if read_lock!(self.memory_manager).true_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_true_prototype(object.clone());

        Ok(())
    }

    fn ins_set_false_prototype(&self, thread: RcThread, _: RcCompiledCode,
                              instruction: &Instruction) -> Result<(), String> {
        if read_lock!(self.memory_manager).false_prototype().is_some() {
            return Err("prototype already defined".to_string());
        }

        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            thread.get_register(slot)
                .ok_or("undefined source object")
        );

        write_lock!(self.memory_manager).set_false_prototype(object.clone());

        Ok(())
    }

    fn ins_set_true(&self, thread: RcThread, _: RcCompiledCode,
                    instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            read_lock!(self.memory_manager)
                .true_object()
                .ok_or("no True object set up".to_string())
        );

        thread.set_register(slot, object);

        Ok(())
    }

    fn ins_set_false(&self, thread: RcThread, _: RcCompiledCode,
                    instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing object slot")
        );

        let object = try!(
            read_lock!(self.memory_manager)
                .false_object()
                .ok_or("no False object set up".to_string())
        );

        thread.set_register(slot, object);

        Ok(())
    }

    fn ins_set_local(&self, thread: RcThread, _: RcCompiledCode,
                     instruction: &Instruction) -> Result<(), String> {
        let local_index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing local variable index".to_string())
        );

        let object_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing object slot".to_string())
        );

        let object = try!(
            thread.get_register(object_index)
                .ok_or("undefined object".to_string())
        );

        thread.set_local(local_index, object);

        Ok(())
    }

    fn ins_get_local(&self, thread: RcThread, _: RcCompiledCode,
                     instruction: &Instruction) -> Result<(), String> {
        let slot_index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index".to_string())
        );

        let local_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing local variable index".to_string())
        );

        let object = try!(
            thread.get_local(local_index)
                .ok_or("undefined local variable index".to_string())
        );

        thread.set_register(slot_index, object);

        Ok(())
    }

    fn ins_set_const(&self, thread: RcThread, code: RcCompiledCode,
                     instruction: &Instruction) -> Result<(), String> {
        let target_slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target object slot".to_string())
        );

        let source_slot = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing source object slot".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing string literal index".to_string())
        );

        let target = try!(
            thread.get_register(target_slot)
                .ok_or("undefined target object".to_string())
        );

        let source = try!(
            thread.get_register(source_slot)
                .ok_or("undefined source object".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        write_lock!(target).add_constant(name.clone(), source);

        Ok(())
    }

    fn ins_get_const(&self, thread: RcThread, code: RcCompiledCode,
                     instruction: &Instruction) -> Result<(), String> {
        let index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index".to_string())
        );

        let src_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing source index".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing string literal index".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        let src = try!(
            thread.get_register(src_index)
                .ok_or("undefined source object".to_string())
        );

        let object = try!(
            read_lock!(src).lookup_constant(name)
                .ok_or(format!("Undefined constant {}", name))
        );

        thread.set_register(index, object);

        Ok(())
    }

    fn ins_set_attr(&self, thread: RcThread, code: RcCompiledCode,
                    instruction: &Instruction) -> Result<(), String> {
        let target_index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target object slot".to_string())
        );

        let source_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing source object slot".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing string literal index".to_string())
        );

        let target_object = try!(
            thread.get_register(target_index)
                .ok_or("undefined target object".to_string())
        );

        let source_object = try!(
            thread.get_register(source_index)
                .ok_or("undefined target object".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        write_lock!(target_object)
            .add_attribute(name.clone(), source_object);

        Ok(())
    }

    fn ins_get_attr(&self, thread: RcThread, code: RcCompiledCode,
                    instruction: &Instruction) -> Result<(), String> {
        let target_index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let source_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing source slot index".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing string literal index".to_string())
        );

        let source = try!(
            thread.get_register(source_index)
                .ok_or("undefined source object".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        let attr = try!(
            read_lock!(source).lookup_attribute(name)
                .ok_or(format!("undefined attribute \"{}\"", name))
        );

        thread.set_register(target_index, attr);

        Ok(())
    }

    fn ins_send(&self, thread: RcThread, code: RcCompiledCode,
                instruction: &Instruction) -> Result<(), String> {
        let result_slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing result slot".to_string())
        );

        let receiver_slot = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing receiver slot".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing string literal index".to_string())
        );

        let allow_private = *try!(
            instruction.arguments
                .get(3)
                .ok_or("missing method visibility".to_string())
        );

        let arg_count = *try!(
            instruction.arguments
                .get(4)
                .ok_or("missing argument count".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        let receiver_lock = try!(
            thread.get_register(receiver_slot)
                .ok_or(format!(
                    "\"{}\" called on an undefined receiver",
                    name
                ))
        );

        let receiver = read_lock!(receiver_lock);

        let method_code = try!(
            receiver.lookup_method(name)
                .ok_or(receiver.undefined_method_error(name))
        );

        if method_code.is_private() && allow_private == 0 {
            return Err(receiver.private_method_error(name));
        }

        let mut arguments = try!(
            self.collect_arguments(thread.clone(), instruction, 5, arg_count)
        );

        if arguments.len() != method_code.required_arguments {
            return Err(format!(
                "\"{}\" requires {} arguments, {} given",
                name,
                method_code.required_arguments,
                arguments.len()
            ));
        }

        // Expose the receiver as "self" to the method
        arguments.insert(0, receiver_lock.clone());

        let retval = try!(
            self.run_code(thread.clone(), method_code, arguments)
        );

        if retval.is_some() {
            thread.set_register(result_slot, retval.unwrap());
        }

        Ok(())
    }

    fn ins_return(&self, thread: RcThread, _: RcCompiledCode,
                  instruction: &Instruction)
                  -> Result<Option<RcObject>, String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing return slot".to_string())
        );

        Ok(thread.get_register(slot))
    }

    fn ins_goto_if_false(&self, thread: RcThread, _: RcCompiledCode,
                         instruction: &Instruction)
                         -> Result<Option<usize>, String> {
        let go_to = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing instruction index".to_string())
        );

        let value_slot = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing value slot".to_string())
        );

        let value   = thread.get_register(value_slot);
        let matched = match value {
            Some(obj) => {
                if read_lock!(obj).truthy() {
                    None
                }
                else {
                    Some(go_to)
                }
            },
            None => { Some(go_to) }
        };

        Ok(matched)
    }

    fn ins_goto_if_true(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction)
                       -> Result<Option<usize>, String> {
        let go_to = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing instruction index".to_string())
        );

        let value_slot = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing value slot".to_string())
        );

        let value   = thread.get_register(value_slot);
        let matched = match value {
            Some(obj) => {
                if read_lock!(obj).truthy() {
                    Some(go_to)
                }
                else {
                    None
                }
            },
            None => { None }
        };

        Ok(matched)
    }

    fn ins_goto(&self, _: RcThread, _: RcCompiledCode,
                instruction: &Instruction) -> Result<usize, String> {
        let go_to = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing instruction index".to_string())
        );

        Ok(go_to)
    }

    fn ins_def_method(&self, thread: RcThread, code: RcCompiledCode,
                      instruction: &Instruction) -> Result<(), String> {
        let receiver_index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing receiver slot".to_string())
        );

        let name_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing string literal index".to_string())
        );

        let code_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing code object index".to_string())
        );

        let receiver_lock = try!(
            thread.get_register(receiver_index)
                .ok_or("undefined receiver".to_string())
        );

        let name = try!(
            code.string_literals
                .get(name_index)
                .ok_or("undefined string literal".to_string())
        );

        let method_code = try!(
            code.code_objects
                .get(code_index)
                .cloned()
                .ok_or("undefined code object index".to_string())
        );

        let mut receiver = write_lock!(receiver_lock);

        receiver.add_method(name.clone(), method_code);

        Ok(())
    }

    fn ins_run_code(&self, thread: RcThread, code: RcCompiledCode,
                    instruction: &Instruction) -> Result<(), String> {
        let result_index = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing result slot".to_string())
        );

        let code_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing code object index".to_string())
        );

        let arg_count = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing argument count".to_string())
        );

        let code_obj = try!(
            code.code_objects
                .get(code_index)
                .cloned()
                .ok_or("undefined code object".to_string())
        );

        let arguments = try!(
            self.collect_arguments(thread.clone(), instruction, 3, arg_count)
        );

        let retval = try!(self.run_code(thread.clone(), code_obj, arguments));

        if retval.is_some() {
            thread.set_register(result_index, retval.unwrap());
        }

        Ok(())
    }

    fn ins_get_toplevel(&self, thread: RcThread, _: RcCompiledCode,
                        instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index")
        );

        let top_level = read_lock!(self.memory_manager).top_level.clone();

        thread.set_register(slot, top_level);

        Ok(())
    }

    fn ins_integer_add(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let added = left_object.value.as_integer() +
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(added), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_div(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() /
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_mul(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() *
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_sub(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() -
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_mod(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() %
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_to_float(&self, thread: RcThread, _: RcCompiledCode,
                       instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let int_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing source slot index".to_string())
        );

        let integer_lock = try!(
            thread.get_register(int_index)
                .ok_or("undefined source object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .float_prototype()
                .ok_or("no Float prototype set up".to_string())
        );

        let integer = read_lock!(integer_lock);

        if !integer.value.is_integer() {
            return Err("source object is not an Integer".to_string());
        }

        let result = integer.value.as_integer() as f64;

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::float(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_to_string(&self, thread: RcThread, _: RcCompiledCode,
                             instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let int_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing source slot index".to_string())
        );

        let integer_lock = try!(
            thread.get_register(int_index)
                .ok_or("undefined source object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .string_prototype()
                .ok_or("no String prototype set up".to_string())
        );

        let integer = read_lock!(integer_lock);

        if !integer.value.is_integer() {
            return Err("source object is not an Integer".to_string());
        }

        let result = integer.value.as_integer().to_string();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::string(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_bitwise_and(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() &
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_bitwise_or(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() |
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_bitwise_xor(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() ^
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_shift_left(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() <<
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_shift_right(&self, thread: RcThread, _: RcCompiledCode,
                               instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let prototype = try!(
            read_lock!(self.memory_manager)
                .integer_prototype()
                .ok_or("no Integer prototype set up".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let result = left_object.value.as_integer() >>
            right_object.value.as_integer();

        let obj = write_lock!(self.memory_manager)
            .allocate(object_value::integer(result), prototype.clone());

        thread.set_register(slot, obj);

        Ok(())
    }

    fn ins_integer_smaller(&self, thread: RcThread, _: RcCompiledCode,
                           instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let smaller = left_object.value.as_integer() <
            right_object.value.as_integer();

        let boolean = if smaller {
            try!(
                read_lock!(self.memory_manager)
                    .true_object()
                    .ok_or("no \"true\" object set up".to_string())
            )
        }
        else {
            try!(
                read_lock!(self.memory_manager)
                    .false_object()
                    .ok_or("no \"false\" object set up".to_string())
            )
        };

        thread.set_register(slot, boolean);

        Ok(())
    }

    fn ins_integer_greater(&self, thread: RcThread, _: RcCompiledCode,
                           instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let smaller = left_object.value.as_integer() >
            right_object.value.as_integer();

        let boolean = if smaller {
            try!(
                read_lock!(self.memory_manager)
                    .true_object()
                    .ok_or("no \"true\" object set up".to_string())
            )
        }
        else {
            try!(
                read_lock!(self.memory_manager)
                    .false_object()
                    .ok_or("no \"false\" object set up".to_string())
            )
        };

        thread.set_register(slot, boolean);

        Ok(())
    }

    fn ins_integer_equal(&self, thread: RcThread, _: RcCompiledCode,
                        instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing target slot index".to_string())
        );

        let left_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing left-hand slot index".to_string())
        );

        let right_index = *try!(
            instruction.arguments
                .get(2)
                .ok_or("missing right-hand slot index".to_string())
        );

        let left_object_lock = try!(
            thread.get_register(left_index)
                .ok_or("undefined left-hand object".to_string())
        );

        let right_object_lock = try!(
            thread.get_register(right_index)
                .ok_or("undefined right-hand object".to_string())
        );

        let left_object  = read_lock!(left_object_lock);
        let right_object = read_lock!(right_object_lock);

        if !left_object.value.is_integer() || !right_object.value.is_integer() {
            return Err("both objects must be integers".to_string());
        }

        let smaller = left_object.value.as_integer() ==
            right_object.value.as_integer();

        let boolean = if smaller {
            try!(
                read_lock!(self.memory_manager)
                    .true_object()
                    .ok_or("no \"true\" object set up".to_string())
            )
        }
        else {
            try!(
                read_lock!(self.memory_manager)
                    .false_object()
                    .ok_or("no \"false\" object set up".to_string())
            )
        };

        thread.set_register(slot, boolean);

        Ok(())
    }

    fn ins_start_thread(&self, thread: RcThread, code: RcCompiledCode,
                        instruction: &Instruction) -> Result<(), String> {
        let slot = *try!(
            instruction.arguments
                .get(0)
                .ok_or("missing slot index".to_string())
        );

        let code_index = *try!(
            instruction.arguments
                .get(1)
                .ok_or("missing code object index".to_string())
        );

        let thread_code = try!(
            code.code_objects
                .get(code_index)
                .cloned()
                .ok_or("undefined code object".to_string())
        );

        try!(
            read_lock!(self.memory_manager)
                .thread_prototype()
                .ok_or("no Thread prototype set up".to_string())
        );

        let thread_object = self.run_thread(thread_code, false);

        thread.set_register(slot, thread_object);

        Ok(())
    }

    fn error(&self, thread: RcThread, message: String) {
        let mut stderr = io::stderr();
        let mut error  = message.to_string();
        let frame      = read_lock!(thread.call_frame);

        *write_lock!(self.exit_status) = Err(());

        frame.each_frame(|frame| {
            error.push_str(&format!(
                "\n{} line {} in \"{}\"",
                frame.file,
                frame.line,
                frame.name
            ));
        });

        write!(&mut stderr, "Fatal error:\n\n{}\n\n", error).unwrap();

        stderr.flush().unwrap();
    }

    fn run_code(&self, thread: RcThread, code: RcCompiledCode,
                args: Vec<RcObject>) -> Result<Option<RcObject>, String> {
        // Scoped so the the RwLock is local to the block, allowing recursive
        // calling of the "run" method.
        {
            thread.push_call_frame(CallFrame::from_code(code.clone()));

            for arg in args.iter() {
                thread.add_local(arg.clone());
            }
        }

        let return_val = try!(self.run(thread.clone(), code));

        thread.pop_call_frame();

        Ok(return_val)
    }

    fn collect_arguments(&self, thread: RcThread, instruction: &Instruction,
                         offset: usize,
                         amount: usize) -> Result<Vec<RcObject>, String> {
        let mut args: Vec<RcObject> = Vec::new();

        for index in offset..(offset + amount) {
            let arg_index = instruction.arguments[index];

            let arg = try!(
                thread.get_register(arg_index)
                    .ok_or(format!("argument {} is undefined", index))
            );

            args.push(arg)
        }

        Ok(args)
    }

    fn run_thread(&self, code: RcCompiledCode, main_thread: bool) -> RcObject {
        let self_clone = self.clone();
        let code_clone = code.clone();

        let (chan_sender, chan_receiver) = channel();

        let handle = thread::spawn(move || {
            let thread_obj: RcObject = chan_receiver.recv().unwrap();
            let vm_thread = read_lock!(thread_obj).value.as_thread();

            let result = self_clone.run(vm_thread.clone(), code_clone);

            write_lock!(self_clone.threads).remove(thread_obj.clone());

            // After this there's a chance thread_obj might be GC'd so we can't
            // reliably use it any more.
            write_lock!(thread_obj).unpin();

            match result {
                Ok(obj) => {
                    vm_thread.set_value(obj);
                },
                Err(message) => {
                    self_clone.error(vm_thread, message);

                    write_lock!(self_clone.threads).stop();
                }
            };
        });

        let vm_thread = Thread::from_code(code.clone(), Some(handle));

        let thread_obj = write_lock!(self.memory_manager)
            .allocate_thread(vm_thread.clone());

        write_lock!(self.threads).add(thread_obj.clone());

        if main_thread {
            vm_thread.set_main();
        }

        chan_sender.send(thread_obj.clone()).unwrap();

        thread_obj
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use call_frame::CallFrame;
    use compiled_code::CompiledCode;
    use instruction::{Instruction, InstructionType};
    use thread::Thread;

    macro_rules! compiled_code {
        ($ins: expr) => (
            CompiledCode::new("test".to_string(), "test".to_string(), 1, $ins)
        );
    }

    macro_rules! call_frame {
        () => (
            CallFrame::new("foo".to_string(), "foo".to_string(), 1)
        );
    }

    macro_rules! instruction {
        ($ins_type: expr, $args: expr) => (
            Instruction::new($ins_type, $args, 1, 1)
        );
    }

    macro_rules! run {
        ($vm: ident, $thread: expr, $cc: expr) => (
            $vm.run($thread.clone(), Arc::new($cc))
        );
    }

    // TODO: test for start()
    // TODO: test for run()

    #[test]
    fn test_ins_set_integer_without_arguments() {
        let vm = VirtualMachine::new();
        let cc = compiled_code!(
            vec![instruction!(InstructionType::SetInteger, Vec::new())]
        );

        let thread = Thread::new(call_frame!(), None);
        let result = run!(vm, thread, cc);

        assert!(result.is_err());
    }

    #[test]
    fn test_ins_set_integer_without_literal_index() {
        let vm = VirtualMachine::new();
        let cc = compiled_code!(
            vec![instruction!(InstructionType::SetInteger, vec![0])]
        );

        let thread = Thread::new(call_frame!(), None);
        let result = run!(vm, thread, cc);

        assert!(result.is_err());
    }

    #[test]
    fn test_ins_set_integer_with_undefined_literal() {
        let vm = VirtualMachine::new();
        let cc = compiled_code!(
            vec![instruction!(InstructionType::SetInteger, vec![0, 0])]
        );

        let thread = Thread::new(call_frame!(), None);
        let result = run!(vm, thread, cc);

        assert!(result.is_err());
    }

    #[test]
    fn test_ins_set_integer_without_integer_prototype() {
        let vm = VirtualMachine::new();

        let mut cc = compiled_code!(
            vec![instruction!(InstructionType::SetInteger, vec![0, 0])]
        );

        cc.add_integer_literal(10);

        let thread = Thread::new(call_frame!(), None);
        let result = run!(vm, thread, cc);

        assert!(result.is_err());
    }

    #[test]
    fn test_ins_set_integer_with_valid_arguments() {
        let vm = VirtualMachine::new();

        let mut cc = compiled_code!(
            vec![
                instruction!(InstructionType::SetObject, vec![0]),
                instruction!(InstructionType::SetIntegerPrototype, vec![0]),
                instruction!(InstructionType::SetInteger, vec![1, 0])
            ]
        );

        cc.add_integer_literal(10);

        let thread = Thread::new(call_frame!(), None);
        let result = run!(vm, thread, cc);

        let int_obj = thread.get_register(1).unwrap();
        let value   = read_lock!(int_obj).value.as_integer();

        assert!(result.is_ok());

        assert_eq!(value, 10);
    }
}
