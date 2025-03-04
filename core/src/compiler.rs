//! The compiler is the last step of the compilation process which translates an AST to Lua code.
//!
//! The compiler module handles the compilation of a list of [`ComplexToken`] ([`Expression`]) into a Lua code.
//! It exposes the [`Compiler`] struct which is used to compile to Lua.

use std::fmt::Write;
use std::iter::{Iterator, Peekable};

use crate::{
	env::{ContinueMode, Options},
	format_clue,
	parser::{CodeBlock, ComplexToken, ComplexToken::*, Expression, FunctionArgs},
	scanner::TokenType::*,
};

/// The Compiler struct is used to compile a list of [`ComplexToken`] ([`Expression`]) into a lua code.
///
/// # Example
/// ```rust
/// use clue_core::{compiler::*, env::Options, parser::*, scanner::*, Clue};
///
/// fn main() -> Result<(), String> {
///     let options = Options::default();
///     let filename = String::from("file.clue");
///     let compiler = Compiler::new(&options, &filename);
///     let code = "local fn a() {return 1;}".to_owned();
///     let clue = Clue::new();
///
///     let (ctokens, _) = clue.parse_code(code)?;
///     let output = compiler.compile_tokens(0, ctokens)?;
///
///     Ok(())
/// }
/// ```
pub struct Compiler<'a> {
	options: &'a Options,
	filename: &'a String,
}

impl<'a> Compiler<'a> {
	/// Creates a new [`Compiler`] instance.
	/// # Example
	/// ```rust
	/// use clue_core::{compiler::Compiler, env::Options};
	///
	/// let options = Options::default();
	/// let compiler = Compiler::new(&options, &String::from("file.clue"));
	/// ```
	pub const fn new(options: &'a Options, filename: &'a String) -> Self {
		Self { options, filename }
	}

	fn indentate(&self, scope: usize) -> String {
		let mut result = String::with_capacity(128);
		for _ in 0..scope {
			result += "\t";
		}
		result
	}

	fn indentate_if<T: Iterator>(&self, ctokens: &mut Peekable<T>, scope: usize) -> String {
		match ctokens.peek() {
			Some(_) => {
				format!("\n{}", self.indentate(scope))
			}
			None => String::with_capacity(4),
		}
	}

	fn compile_list<T>(
		&self,
		list: Vec<T>,
		separator: &str,
		tostring: &mut impl FnMut(T) -> Result<String, String>,
	) -> Result<String, String> {
		let mut result = String::new();
		let end = list.len();
		let mut start = 0usize;
		for element in list {
			result += &(tostring(element)?);
			start += 1;
			if start < end {
				result += separator
			}
		}
		Ok(result)
	}

	fn compile_identifiers(&self, names: Vec<String>) -> Result<String, String> {
		self.compile_list(names, ", ", &mut Ok)
	}

	fn compile_expressions(&self, scope: usize, values: Vec<Expression>) -> Result<String, String> {
		self.compile_list(values, ", ", &mut |expr| {
			self.compile_expression(scope, expr)
		})
	}

	fn compile_function(
		&self,
		scope: usize,
		args: FunctionArgs,
		code: CodeBlock,
	) -> Result<(String, String), String> {
		let mut code = self.compile_code_block(scope + self.options.env_debug as usize, "", code)?;
		let args = self.compile_list(args, ", ", &mut |(arg, default)| {
			if let Some((default, line)) = default {
				let default = self.compile_expression(scope + 2, default)?;
				let pre = self.indentate(scope + 1);
				let debug = self.compile_debug_line(line, scope + 2, true);
				let line = self.compile_debug_comment(line);
				code = format_clue!(
					"\n",
					pre,
					"if ",
					arg,
					" == nil then\n",
					pre,
					"\t",
					debug,
					arg,
					" = ",
					default,
					line,
					"\n",
					pre,
					"end",
					code
				);
			}
			Ok(arg)
		})?;
		if self.options.env_debug {
			let pre = self.indentate(scope);
			code = format_clue!(
				"\n",
				pre,
				"\tlocal _result = {xpcall(function(",
				args,
				")",
				code,
				"end, function(err)\n",
				pre,
				"\t\t_errored_file = \"",
				self.filename,
				"\"\n",
				pre,
				"\t\t_clue_error(err)\n",
				pre,
				"\tend",
				if args.is_empty() {
					String::new()
				} else {
					format_clue!(", ", args)
				},
				")}\n",
				pre,
				"\tlocal _ok = table.remove(_result, 1)\n",
				pre,
				"\tif _errored then\n",
				pre,
				"\t\tlocal err, caller = _errored, debug.getinfo(2, \"f\").func\n",
				pre,
				"\t\tif caller == pcall or caller == xpcall then _errored = nil end\n",
				pre,
				"\t\terror(err)\n",
				pre,
				"\tend\n",
				pre,
				"\treturn (unpack or table.unpack)(_result)\n",
				pre
			)
		}
		Ok((code, args))
	}

	fn compile_code_block(
		&self,
		scope: usize,
		start: &str,
		block: CodeBlock,
	) -> Result<String, String> {
		let pre = self.indentate(scope);
		let code = self.compile_tokens(scope + 1, block.code)?;
		let debug = self.compile_debug_line(block.start, scope + 1, true);
		Ok(if self.options.env_debug {
			format!(
				"{}\n{}\t{}--{}->{}\n{}\n{}",
				start, pre, debug, block.start, block.end, code, pre
			)
		} else {
			format_clue!(start, "\n", code, "\n", pre)
		})
	}

	fn compile_debug_comment(&self, line: usize) -> String {
		if self.options.env_debug {
			format!(" --{line}")
		} else {
			String::new()
		}
	}

	fn compile_debug_line(&self, line: usize, scope: usize, indentate_last: bool) -> String {
		if self.options.env_debug {
			let debug = format_clue!("_clueline = ", line.to_string(), ";");
			if indentate_last {
				format_clue!(debug, "\n", self.indentate(scope))
			} else {
				format_clue!("\n", self.indentate(scope), debug)
			}
		} else {
			String::new()
		}
	}

	fn compile_identifier(&self, scope: usize, expr: Expression) -> Result<String, String> {
		let mut result = String::with_capacity(32);
		for t in expr {
			result += &match t {
				SYMBOL(lexeme) => lexeme,
				EXPR(expr) => self.compile_expression(scope, expr)?,
				CALL(args) => {
					format_clue!("(", self.compile_expressions(scope, args.clone())?, ")")
				}
				_ => return Err(String::from("Unexpected ComplexToken found")),
			}
		}
		Ok(result)
	}

	fn compile_expression(&self, mut scope: usize, expr: Expression) -> Result<String, String> {
		let mut result = String::with_capacity(64);
		for t in expr {
			result += &match t {
				SYMBOL(lexeme) => lexeme,
				TABLE {
					values,
					metas,
					metatable,
				} => {
					scope += 1;
					let mut prevline = 0;
					let pre1 = self.indentate(scope);
					let values = if values.is_empty() {
						String::new()
					} else {
						self.compile_list(values, ", ", &mut |(name, value, line)| {
							let value = self.compile_expression(scope, value)?;
							let l = if prevline != 0 {
								self.compile_debug_comment(prevline)
							} else {
								String::new()
							};
							prevline = line;
							if let Some(name) = name {
								let name = self.compile_expression(scope, name)?;
								Ok(format_clue!(l, "\n", pre1, name, " = ", value))
							} else {
								Ok(format_clue!(l, "\n", pre1, value))
							}
						})? + &self.compile_debug_comment(prevline)
							+ "\n"
					};
					prevline = 0;
					let pre2 = self.indentate(scope - 1);
					if metas.is_empty() {
						scope -= 1;
						if let Some(metatable) = metatable {
							format!("setmetatable({{{values}{pre2}}}, {metatable})")
						} else {
							format!("{{{values}{pre2}}}")
						}
					} else {
						let metas =
							self.compile_list(metas, ", ", &mut |(name, value, line)| {
								let value = self.compile_expression(scope, value)?;
								let l = if prevline != 0 {
									self.compile_debug_comment(prevline)
								} else {
									String::new()
								};
								prevline = line;
								Ok(format_clue!(l, "\n", pre1, name, " = ", value))
							})?;
						scope -= 1;
						let line = self.compile_debug_comment(prevline);
						format!("setmetatable({{{values}{pre2}}}, {{{metas}{line}\n{pre2}}})",)
					}
				}
				LAMBDA { args, code } => {
					let (code, args) = self.compile_function(scope, args, code)?;
					format_clue!("function(", args, ")", code, "end")
				}
				IDENT { expr, .. } => self.compile_identifier(scope, expr)?,
				CALL(args) => format!("({})", self.compile_expressions(scope, args)?),
				EXPR(expr) => format!("({})", self.compile_expression(scope, expr)?),
				_ => return Err(String::from("Unexpected ComplexToken found")),
			}
		}
		Ok(result)
	}

	fn compile_elseif_chain(
		&self,
		scope: usize,
		condition: Expression,
		code: CodeBlock,
		next: Option<Box<ComplexToken>>,
	) -> Result<String, String> {
		let condition = self.compile_expression(scope, condition)?;
		let code = self.compile_code_block(scope, "then", code)?;
		let next = if let Some(next) = next {
			String::from("else")
				+ &match *next {
					IF_STATEMENT {
						condition,
						code,
						next,
					} => self.compile_elseif_chain(scope, condition, code, next)?,
					DO_BLOCK(code) => self.compile_code_block(scope, "", code)?,
					_ => return Err(String::from("Unexpected ComplexToken found")),
				}
		} else {
			String::new()
		};
		Ok(format_clue!("if ", condition, " ", code, next))
	}

	/// Compiles an [`Expression`] into a [`String`] of Lua.
	///
	/// # Errors
	/// Returns an error if an unexpected [`ComplexToken`] is found.
	///
	/// # Example
	/// ```rust
	/// use clue_core::{compiler::*, env::Options, parser::*, scanner::*, Clue};
	///
	/// fn main() -> Result<(), String> {
	///     let options = Options::default();
	///     let filename = String::from("file.clue");
	///     let compiler = Compiler::new(&options, &filename);
	///     let code = "local fn a() {return 1;}".to_owned();
	///     let clue = Clue::new();
	///
	///     let (ctokens, _) = clue.parse_code(code)?;
	///     let output = compiler.compile_tokens(0, ctokens)?;
	///
	///     Ok(())
	/// }
	/// ```
	pub fn compile_tokens(&self, scope: usize, ctokens: Expression) -> Result<String, String> {
		let mut result = self.indentate(scope);
		let ctokens = &mut ctokens.into_iter().peekable();
		while let Some(t) = ctokens.next() {
			result += &match t {
				SYMBOL(lexeme) => lexeme,
				VARIABLE {
					local,
					names,
					values,
					line,
				} => {
					let debug = self.compile_debug_line(line, scope, true);
					let line = self.compile_debug_comment(line);
					if !local && self.options.env_rawsetglobals {
						let mut result = debug;
						let mut valuesit = values.iter();
						let namesit = &mut names.iter().peekable();
						while let Some(name) = namesit.next() {
							let value = if let Some(value) = valuesit.next() {
								self.compile_expression(scope, value.clone())?
							} else {
								String::from("nil")
							};
							let end = {
								let pend = self.indentate_if(namesit, scope);
								if !pend.is_empty() {
									pend
								} else {
									self.indentate_if(ctokens, scope)
								}
							};
							write!(result, "rawset(_G, \"{name}\", {value});{line}{end}")
								.map_err(|e| e.to_string())?
						}
						result
					} else {
						let end = self.indentate_if(ctokens, scope);
						let pre = if local { "local " } else { "" };
						if values.is_empty() {
							let ident = self.compile_identifiers(names)?;
							format_clue!(debug, pre, ident, ";", line, end)
						} else {
							let values = self.compile_expressions(scope, values)?;
							let names = self.compile_identifiers(names)?;
							format_clue!(debug, pre, names, " = ", values, ";", line, end)
						}
					}
				}
				ALTER {
					kind,
					names,
					values,
					line,
				} => {
					let iter = names.into_iter();
					let mut names: Vec<String> = Vec::new();
					for name in iter {
						names.push(self.compile_expression(scope, name)?)
					}
					let mut i = 0usize;
					let values = self.compile_list(values, ", ", &mut |expr| {
						let name = if let Some(name) = names.get(i) {
							name.clone()
						} else {
							String::from("nil")
						};
						i += 1;
						Ok((if kind == DEFINE {
							String::new()
						} else {
							name + match kind {
								DEFINE_AND => " and ",
								DEFINE_OR => " or ",
								INCREASE => " + ",
								DECREASE => " - ",
								MULTIPLY => " * ",
								DIVIDE => " / ",
								EXPONENTIATE => " ^ ",
								CONCATENATE => " .. ",
								MODULATE => " % ",
								_ => return Err(String::from("Unexpected alter type found")),
							}
						}) + &self.compile_expression(scope, expr)?)
					})?;
					let names = self.compile_identifiers(names)?;
					let debug = self.compile_debug_line(line, scope, true);
					let line = self.compile_debug_comment(line);
					format_clue!(
						debug,
						names,
						" = ",
						values,
						";",
						line,
						self.indentate_if(ctokens, scope)
					)
				}
				FUNCTION {
					local,
					name,
					args,
					code,
				} => {
					let pre = if local { "local " } else { "" };
					let end = self.indentate_if(ctokens, scope);
					let name = self.compile_expression(scope, name)?;
					let (code, args) = self.compile_function(scope, args, code)?;
					format_clue!(pre, "function ", name, "(", args, ")", code, "end", end)
				}
				IF_STATEMENT {
					condition,
					code,
					next,
				} => {
					let code = self.compile_elseif_chain(scope, condition, code, next)?;
					format_clue!(code, "end", self.indentate_if(ctokens, scope))
				}
				MATCH_BLOCK {
					name,
					value,
					branches,
					line,
				} => {
					let value = self.compile_expression(scope, value)?;
					let debug = self.compile_debug_line(line, scope, true);
					let line = self.compile_debug_comment(line);
					let branches = {
						let mut result = self.indentate(scope);
						let last = branches.len() - 1;
						let branches = branches.into_iter().enumerate();
						for (i, (conditions, internal_expr, extraif, code)) in branches {
							let empty = conditions.is_empty();
							let default = empty && extraif.is_none();
							let condition = {
								let mut condition =
									self.compile_list(conditions, "or ", &mut |expr| {
										let expr = self.compile_expression(scope, expr)?;
										Ok(format_clue!("(", name, " == ", expr, ") "))
									})?;
								format_clue!("if ", if let Some(extraif) = extraif {
									condition.pop();
									let extraif = self.compile_expression(scope, extraif)?;
									if empty {
										extraif + " "
									} else {
										format_clue!("(", condition, ") and ", extraif, " ")
									}
								} else {
									condition
								}, "then")
							};
							let end = if i >= last {
								"end"
							} else {
								"else"
							};
							let pre = self.indentate(scope + i);
							let internal_code = if internal_expr.is_empty() {
								None
							} else {
								Some(self.compile_tokens(scope + i, internal_expr)?)
							};
							let code = if i == 0 {
								let code = self.compile_code_block(
									scope,
									&condition,
									code,
								)? + end;
								if let Some(internal_code) = internal_code {
									format_clue!(
										internal_code,
										'\n',
										pre,
										code
									)
								} else {
									code
								}
							} else if default {
								self.compile_code_block(
									scope + i - 1,
									"",
									code,
								)? + end
							} else {
								let code = self.compile_code_block(
									scope + i,
									&condition,
									code,
								)?;
								let mut code = format_clue!(
									'\n',
									pre,
									code,
									end
								);
								if let Some(internal_code) = internal_code {
									code = format_clue!(
										'\n',
										internal_code,
										code
									)
								}
								if i >= last {
									code += &format_clue!('\n', self.indentate(scope + i - 1), "end");
								}
								code
							};
							result += &code;
						}
						if last > 1 {
							result.push('\n');
							for i in (1..last - 1).rev() {
								result += &(self.indentate(scope + i) + "end\n");
							}
							format_clue!(result, self.indentate(scope), "end")
						} else {
							result
						}
					};
					let end = self.indentate_if(ctokens, scope);
					format_clue!(
						debug, "local ", name, " = ", value, ';', line, '\n', branches, end
					)
				}
				WHILE_LOOP { condition, code, line } => {
					let condition = self.compile_expression(scope, condition)?;
					let debug = self.compile_debug_line(line, scope, true);
					let code = self.compile_code_block(scope, "do", code)?;
					format_clue!(
						debug,
						"while ",
						condition,
						" ",
						code,
						debug,
						"end",
						self.indentate_if(ctokens, scope)
					)
				}
				LOOP_UNTIL { condition, code, line } => {
					let condition = self.compile_expression(scope, condition)?;
					let debug = self.compile_debug_line(line, scope, true);
					let code = self.compile_code_block(scope, "", code)?;
					format_clue!(
						"repeat ",
						code,
						debug,
						"until ",
						condition,
						self.indentate_if(ctokens, scope)
					)
				}
				FOR_LOOP {
					iterator,
					start,
					end,
					alter,
					code,
					line,
				} => {
					let start = self.compile_expression(scope, start)?;
					let endexpr = self.compile_expression(scope, end)?;
					let alter = self.compile_expression(scope, alter)?;
					let debug = self.compile_debug_line(line, scope, true);
					let code = self.compile_code_block(scope, "do", code)?;
					let end = self.indentate_if(ctokens, scope);
					format_clue!(
						debug,
						"for ",
						iterator,
						" = ",
						start,
						", ",
						endexpr,
						", ",
						alter,
						" ",
						code,
						debug,
						"end",
						end
					)
				}
				FOR_FUNC_LOOP {
					iterators,
					expr,
					code,
					line,
				} => {
					let expr = self.compile_expression(scope, expr)?;
					let iterators = self.compile_identifiers(iterators)?;
					let debug = self.compile_debug_line(line, scope, true);
					let code = self.compile_code_block(scope, "do", code)?;
					format_clue!(
						debug,
						"for ",
						iterators,
						" in ",
						expr,
						" ",
						code,
						debug,
						"end",
						self.indentate_if(ctokens, scope)
					)
				}
				TRY_CATCH {
					totry,
					error,
					catch,
				} => {
					let i = self.indentate_if(ctokens, scope);
					let totry = self.compile_code_block(scope, "function()", totry)?;
					if let Some(catch) = catch {
						let catch = self.compile_code_block(scope, "if not _check then", catch)?;
						let i2 = self.indentate(scope);
						if let Some(error) = error {
							format_clue!(
								"local _check, ",
								error,
								" = pcall(",
								totry,
								"end)\n",
								i2,
								catch,
								"end",
								i
							)
						} else {
							format_clue!(
								"local _check = pcall(",
								totry,
								"end)\n",
								i2,
								catch,
								"end",
								i
							)
						}
					} else {
						format_clue!("pcall(", totry, "end)", i)
					}
				}
				IDENT { expr, line } => {
					let expr = self.compile_identifier(scope, expr)?;
					let debug = self.compile_debug_line(line, scope, true);
					let line = self.compile_debug_comment(line);
					format_clue!(debug, expr, ";", line, self.indentate_if(ctokens, scope))
				}
				EXPR(expr) => {
					format!("({})", self.compile_expression(scope, expr)?)
				}
				DO_BLOCK(code) => {
					format!(
						"{}end{}",
						self.compile_code_block(scope, "do", code)?,
						self.indentate_if(ctokens, scope)
					)
				}
				RETURN_EXPR(exprs) => {
					if let Some(exprs) = exprs {
						format!("return {};", self.compile_expressions(scope, exprs)?)
					} else {
						String::from("return;")
					}
				}
				CONTINUE_LOOP => {
					let end = self.indentate_if(ctokens, scope);
					format!(
						"{};{}",
						if matches!(
							self.options.env_continue,
							ContinueMode::LuaJIT | ContinueMode::Goto
						) {
							"goto continue"
						} else {
							"continue"
						},
						end
					)
				}
				BREAK_LOOP => String::from("break;") + &self.indentate_if(ctokens, scope),
				_ => return Err(String::from("Unexpected ComplexToken found")),
			}
		}
		Ok(result)
	}
}
