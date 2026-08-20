use std::fmt::{Arguments, Display, Write as _};

use crate::{Instruction, ProgramBytecode};

const OP_WIDTH: usize = 10;
const OPERAND_WIDTH: usize = 4;
const HEADER_WIDTH: usize = 7; // "XXXX | " = 7 chars

fn append(out: &mut String, args: Arguments<'_>) {
    out.write_fmt(args)
        .expect("Writing to string should not fail.");
}

#[must_use]
pub fn format_bytecode(program: &ProgramBytecode) -> String {
    let mut out = String::new();
    for (chunk_id, chunk) in program.chunks.iter().enumerate() {
        append(&mut out, format_args!("== Chunk {chunk_id} ==\n"));
        for (offset, instruction) in chunk.code.iter().enumerate() {
            write_header(&mut out, offset);
            format_instruction(instruction, offset, program, &mut out);
            out.push('\n');
        }
    }
    out
}

fn write_header(out: &mut String, offset: usize) {
    append(out, format_args!("{offset:04} | "));
}

fn write_op(out: &mut String, name: &str) {
    out.push_str(name);
}

fn pad_op(out: &mut String, name: &str) {
    append(out, format_args!("{name:<OP_WIDTH$}"));
}

fn write_operand(out: &mut String, val: impl Display) {
    append(out, format_args!(" {val:>OPERAND_WIDTH$}"));
}

fn write_value(out: &mut String, data: Option<&l3_runtime::HeapData>) {
    match data {
        Some(data) => append(out, format_args!(" ({data})")),
        None => out.push_str(" ?"),
    }
}

fn write_compare(out: &mut String, name: &str, keep_rhs: bool) {
    write_op(out, name);
    if keep_rhs {
        out.push_str(" keep rhs");
    }
}

fn format_instruction(
    inst: &Instruction,
    offset: usize,
    program: &ProgramBytecode,
    out: &mut String,
) {
    match inst {
        // No operands
        Instruction::Return => write_op(out, "RETURN"),
        Instruction::Add => write_op(out, "ADD"),
        Instruction::Subtract => write_op(out, "SUBTRACT"),
        Instruction::Multiply => write_op(out, "MULTIPLY"),
        Instruction::Divide => write_op(out, "DIVIDE"),
        Instruction::Modulo => write_op(out, "MODULO"),
        Instruction::Power => write_op(out, "POWER"),
        Instruction::Negate => write_op(out, "NEGATE"),
        Instruction::Not => write_op(out, "NOT"),
        Instruction::GetIndex => write_op(out, "GET_INDEX"),
        Instruction::SetIndex => write_op(out, "SET_INDEX"),

        // Single operand
        Instruction::Pop { count } => {
            pad_op(out, "POP");
            write_operand(out, count);
        },
        Instruction::Duplicate { index } => {
            pad_op(out, "DUPLICATE");
            write_operand(out, index);
        },
        Instruction::GetLocal { index } => {
            pad_op(out, "GET_LOCAL");
            write_operand(out, index.0);
        },
        Instruction::SetLocal { index } => {
            pad_op(out, "SET_LOCAL");
            write_operand(out, index.0);
        },
        Instruction::Jump {
            offset: jump_offset,
        } => {
            pad_op(out, "JUMP");
            write_operand(out, jump_offset.0);
        },
        Instruction::MakeArray { count } => {
            pad_op(out, "MAKE_ARRAY");
            write_operand(out, count);
        },
        Instruction::VectorAppend { count } => {
            pad_op(out, "VECTOR_APPEND");
            write_operand(out, count);
        },
        Instruction::GetUpvalue { index } => {
            pad_op(out, "GET_UPVALUE");
            write_operand(out, index.0);
        },
        Instruction::SetUpvalue { index } => {
            pad_op(out, "SET_UPVALUE");
            write_operand(out, index.0);
        },

        // Index + value
        Instruction::Constant { index } => {
            pad_op(out, "CONSTANT");
            write_operand(out, index.0);
            write_value(out, program.constants.get(*index));
        },

        // Named global
        Instruction::GetGlobal { name_index } => {
            pad_op(out, "GET_GLOBAL");
            write_operand(out, name_index.0);
            append(
                out,
                format_args!(
                    " '{}'",
                    display_constant(program.constants.get(*name_index))
                ),
            );
        },
        Instruction::SetGlobal { name_index } => {
            pad_op(out, "SET_GLOBAL");
            write_operand(out, name_index.0);
            append(
                out,
                format_args!(
                    " '{}'",
                    display_constant(program.constants.get(*name_index))
                ),
            );
        },

        // Comparisons with optional keep_rhs suffix
        Instruction::Equal { keep_rhs } => write_compare(out, "EQUAL", *keep_rhs),
        Instruction::NotEqual { keep_rhs } => write_compare(out, "NOT_EQUAL", *keep_rhs),
        Instruction::Greater { keep_rhs } => write_compare(out, "GREATER", *keep_rhs),
        Instruction::GreaterEqual { keep_rhs } => write_compare(out, "GREATER_EQUAL", *keep_rhs),
        Instruction::Less { keep_rhs } => write_compare(out, "LESS", *keep_rhs),
        Instruction::LessEqual { keep_rhs } => write_compare(out, "LESS_EQUAL", *keep_rhs),

        // Call
        Instruction::Call {
            arg_count,
            keep_return_value,
        } => {
            pad_op(out, "CALL");
            write_operand(out, arg_count);
            append(out, format_args!(" {keep_return_value}"));
        },

        // For loop
        Instruction::ForLoop {
            control_index,
            limit_index,
            body_offset,
            inclusive,
            step_index,
        } => {
            pad_op(out, "FOR_LOOP");
            let ctrl = control_index.0;
            let lim = limit_index.0;
            let body = body_offset.0;
            append(out, format_args!(" ctrl={ctrl:>OPERAND_WIDTH$}"));
            append(out, format_args!(" lim={lim:>OPERAND_WIDTH$}"));
            append(out, format_args!(" body={body:>OPERAND_WIDTH$}"));
            let cmp = if *inclusive { "LE" } else { "LT" };
            append(out, format_args!(" {cmp}"));
            match step_index {
                Some(si) => {
                    let s = si.0;
                    append(out, format_args!(" step={s:>OPERAND_WIDTH$}"));
                },
                None => out.push_str(" step=const1"),
            }
        },

        // Jump-if
        Instruction::JumpIf {
            offset: jump_offset,
            expected,
            keep_stay,
            keep_jump,
        } => {
            pad_op(out, "JUMP_IF");
            write_operand(out, jump_offset.0);
            append(out, format_args!(" {expected}"));
            if *keep_jump || *keep_stay {
                out.push_str(" keep after");
            }
            if *keep_jump {
                out.push_str(" jump");
            }
            if *keep_stay {
                out.push_str(" stay");
            }
        },

        // Closure with upvalue continuation lines
        Instruction::Closure {
            function_index,
            upvalues,
        } => {
            pad_op(out, "CLOSURE");
            write_operand(out, function_index.0);
            append(
                out,
                format_args!(
                    " ({})",
                    display_constant(program.constants.get(*function_index))
                ),
            );
            let continuation_indent = HEADER_WIDTH + OP_WIDTH + 1;
            for uv in upvalues {
                let kind = if uv.is_local { "local" } else { "upvalue" };
                out.push('\n');
                append(out, format_args!("{offset:04} | "));
                append(
                    out,
                    format_args!("{:<width$}", "", width = continuation_indent - HEADER_WIDTH),
                );
                append(out, format_args!("{kind} {}", uv.index));
            }
        },
    }
}

fn display_constant(data: Option<&l3_runtime::HeapData>) -> String {
    data.map_or_else(|| "?".to_string(), |data| format!("{data}"))
}
