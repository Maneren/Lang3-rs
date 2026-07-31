use crate::{Instruction, ProgramBytecode};
use std::fmt::{Display, Write};

const OP_WIDTH: usize = 10;
const OPERAND_WIDTH: usize = 4;
const HEADER_WIDTH: usize = 7; // "XXXX | " = 7 chars

#[must_use]
pub fn format_bytecode(program: &ProgramBytecode) -> String {
    let mut out = String::new();
    for (chunk_id, chunk) in program.chunks.iter().enumerate() {
        writeln!(out, "== Chunk {chunk_id} ==").unwrap();
        for (offset, instruction) in chunk.code.iter().enumerate() {
            write_header(&mut out, offset);
            format_instruction(instruction, offset, program, &mut out);
            writeln!(out).unwrap();
        }
    }
    out
}

fn write_header(out: &mut String, offset: usize) {
    write!(out, "{offset:04} | ").unwrap();
}

fn write_op(out: &mut String, name: &str) {
    out.push_str(name);
}

fn pad_op(out: &mut String, name: &str) {
    write!(out, "{name:<OP_WIDTH$}").unwrap();
}

fn write_operand(out: &mut String, val: impl Display) {
    write!(out, " {val:>OPERAND_WIDTH$}").unwrap();
}

fn write_value(out: &mut String, cell: &l3_runtime::HeapCell) {
    write!(out, " ({})", cell.value).unwrap();
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
        }
        Instruction::Duplicate { index } => {
            pad_op(out, "DUPLICATE");
            write_operand(out, index);
        }
        Instruction::GetLocal { index } => {
            pad_op(out, "GET_LOCAL");
            write_operand(out, index);
        }
        Instruction::SetLocal { index } => {
            pad_op(out, "SET_LOCAL");
            write_operand(out, index);
        }
        Instruction::Jump {
            offset: jump_offset,
        } => {
            pad_op(out, "JUMP");
            write_operand(out, jump_offset);
        }
        Instruction::MakeArray { count } => {
            pad_op(out, "MAKE_ARRAY");
            write_operand(out, count);
        }
        Instruction::GetUpvalue { index } => {
            pad_op(out, "GET_UPVALUE");
            write_operand(out, index);
        }
        Instruction::SetUpvalue { index } => {
            pad_op(out, "SET_UPVALUE");
            write_operand(out, index);
        }

        // Index + value
        Instruction::Constant { index } => {
            pad_op(out, "CONSTANT");
            write_operand(out, index);
            write_value(out, &program.constants[*index]);
        }

        // Named global
        Instruction::GetGlobal { name_index } | Instruction::SetGlobal { name_index } => {
            let name = match inst {
                Instruction::GetGlobal { .. } => "GET_GLOBAL",
                Instruction::SetGlobal { .. } => "SET_GLOBAL",
                _ => unreachable!(),
            };
            pad_op(out, name);
            write_operand(out, name_index);
            write!(
                out,
                " '{}'",
                display_constant(&program.constants[*name_index])
            )
            .unwrap();
        }

        // Comparisons with optional keep_rhs suffix
        Instruction::Equal { keep_rhs }
        | Instruction::NotEqual { keep_rhs }
        | Instruction::Greater { keep_rhs }
        | Instruction::GreaterEqual { keep_rhs }
        | Instruction::Less { keep_rhs }
        | Instruction::LessEqual { keep_rhs } => {
            let name = match inst {
                Instruction::Equal { .. } => "EQUAL",
                Instruction::NotEqual { .. } => "NOT_EQUAL",
                Instruction::Greater { .. } => "GREATER",
                Instruction::GreaterEqual { .. } => "GREATER_EQUAL",
                Instruction::Less { .. } => "LESS",
                Instruction::LessEqual { .. } => "LESS_EQUAL",
                _ => unreachable!(),
            };
            write_op(out, name);
            if *keep_rhs {
                write!(out, " keep rhs").unwrap();
            }
        }

        // Call
        Instruction::Call {
            arg_count,
            keep_return_value,
        } => {
            pad_op(out, "CALL");
            write_operand(out, arg_count);
            write!(out, " {keep_return_value}").unwrap();
        }

        // For loop
        Instruction::ForLoop {
            control_index,
            limit_index,
            body_offset,
            inclusive,
            step_index,
        } => {
            pad_op(out, "FOR_LOOP");
            write!(out, " ctrl={control_index:>OPERAND_WIDTH$}").unwrap();
            write!(out, " lim={limit_index:>OPERAND_WIDTH$}").unwrap();
            write!(out, " body={body_offset:>OPERAND_WIDTH$}").unwrap();
            let cmp = if *inclusive { "LE" } else { "LT" };
            write!(out, " {cmp}").unwrap();
            match step_index {
                Some(si) => write!(out, " step={si:>OPERAND_WIDTH$}").unwrap(),
                None => write!(out, " step=const1").unwrap(),
            }
        }

        // Jump-if
        Instruction::JumpIf {
            offset: jump_offset,
            expected,
            keep_stay,
            keep_jump,
        } => {
            pad_op(out, "JUMP_IF");
            write_operand(out, jump_offset);
            write!(out, " {expected}").unwrap();
            if *keep_jump || *keep_stay {
                write!(out, " keep after").unwrap();
            }
            if *keep_jump {
                write!(out, " jump").unwrap();
            }
            if *keep_stay {
                write!(out, " stay").unwrap();
            }
        }

        // Closure with upvalue continuation lines
        Instruction::Closure {
            function_index,
            upvalues,
        } => {
            pad_op(out, "CLOSURE");
            write_operand(out, function_index);
            write!(
                out,
                " ({})",
                display_constant(&program.constants[*function_index])
            )
            .unwrap();
            let continuation_indent = HEADER_WIDTH + OP_WIDTH + 1;
            for uv in upvalues {
                let kind = if uv.is_local { "local" } else { "upvalue" };
                writeln!(out).unwrap();
                write!(out, "{offset:04} | ").unwrap();
                write!(
                    out,
                    "{:<width$}",
                    "",
                    width = continuation_indent - HEADER_WIDTH
                )
                .unwrap();
                write!(out, "{kind} {}", uv.index).unwrap();
            }
        }
    }
}

fn display_constant(cell: &l3_runtime::HeapCell) -> String {
    format!("{}", cell.value)
}
