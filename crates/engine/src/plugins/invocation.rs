//! Choosing how one call runs a program: the invocation its arguments and
//! environment fit, and the Java major a managed-Java program runs on. A pure
//! function of the declared program and the call, so whatever runs programs
//! applies the same rules.

use proto::plugin::{Invocation, InvocationMismatch, JavaChoiceError, Program};

use super::process::ProgramError;

/// What a call says about how it wants a program run.
pub(crate) struct Call<'a> {
    pub args: &'a [String],
    pub env: &'a [&'a str],
    /// The major the call asked for; only a program declared with a range takes one.
    pub java_major: Option<u32>,
    /// The major the host uses for the call's game version, when it has one.
    pub game_java: Option<u32>,
}

/// How a program will run: the invocation that fits and, for a managed Java
/// program, the major.
#[derive(Debug)]
pub(crate) struct Selection {
    pub invocation: Invocation,
    pub java: Option<u32>,
}

/// The invocation of `program` that `call` fits and the Java major it runs on.
/// A refusal says which rule failed, never what the arguments were.
pub(crate) fn select(program: &Program, call: &Call<'_>) -> Result<Selection, ProgramError> {
    let invocation = program.matching(call.args, call.env).map_err(|mismatch| {
        ProgramError::Denied(match mismatch {
            InvocationMismatch::Args => {
                format!("the arguments are outside what `{}` declares", program.id)
            }
            InvocationMismatch::Env(name) => format!(
                "`{}` does not declare the environment variable `{name}` for these arguments",
                program.id
            ),
        })
    })?;
    let java =
        match (&program.java, call.java_major) {
            (Some(runtime), asked) => Some(runtime.choose(asked, call.game_java).map_err(
                |error| match error {
                    JavaChoiceError::OutsideRange { .. } => ProgramError::Denied(error.to_string()),
                    _ => ProgramError::Invalid(error.to_string()),
                },
            )?),
            (None, Some(_)) => {
                return Err(ProgramError::Invalid(format!(
                    "`{}` does not run on a Hestia-managed Java, so the call names no major",
                    program.id
                )));
            }
            (None, None) => None,
        };
    Ok(Selection { invocation, java })
}

#[cfg(test)]
mod tests {
    use proto::plugin::{JavaProfile, JavaRange, ProgramJava};

    use super::*;

    fn tokens(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| token.to_string()).collect()
    }

    fn named(name: &str, args: &[&str]) -> Invocation {
        Invocation {
            name: name.into(),
            args: tokens(args),
            ..Invocation::default()
        }
    }

    fn buildtools() -> Program {
        Program {
            id: "buildtools".into(),
            java: Some(ProgramJava::Range(JavaRange { min: 17, max: 21 })),
            invocations: vec![
                named("build", &["-jar", "bt.jar", "--rev", "*"]),
                Invocation {
                    env: ["TOKEN".to_string()].into(),
                    ..named("publish", &["-jar", "bt.jar", "publish"])
                },
            ],
            ..Program::default()
        }
    }

    fn call<'a>(args: &'a [String], env: &'a [&'a str], java: Option<u32>) -> Call<'a> {
        Call {
            args,
            env,
            java_major: java,
            game_java: None,
        }
    }

    #[test]
    fn the_first_invocation_the_call_fits_is_selected() {
        let program = buildtools();
        let build = tokens(&["-jar", "bt.jar", "--rev", "1.21"]);
        let publish = tokens(&["-jar", "bt.jar", "publish"]);

        let selected = select(&program, &call(&build, &[], Some(21))).unwrap();
        assert_eq!(selected.invocation.name, "build");
        assert_eq!(selected.java, Some(21));
        let selected = select(&program, &call(&publish, &["TOKEN"], Some(17))).unwrap();
        assert_eq!(selected.invocation.name, "publish");
        assert_eq!(selected.java, Some(17));
    }

    #[test]
    fn arguments_or_variables_no_invocation_declares_are_denied() {
        let program = buildtools();
        let outside = tokens(&["-jar", "other.jar"]);
        let publish = tokens(&["-jar", "bt.jar", "publish"]);

        assert!(matches!(
            select(&program, &call(&outside, &[], Some(21))),
            Err(ProgramError::Denied(message)) if message.contains("outside what `buildtools` declares")
        ));
        assert!(matches!(
            select(&program, &call(&publish, &["OTHER"], Some(21))),
            Err(ProgramError::Denied(message)) if message.contains("`OTHER`")
        ));
        assert!(matches!(
            select(&program, &call(&publish, &[], None)),
            Err(ProgramError::Invalid(_))
        ));
    }

    #[test]
    fn a_java_major_outside_the_declared_range_is_denied() {
        let program = buildtools();
        let build = tokens(&["-jar", "bt.jar", "--rev", "1.21"]);
        for refused in [8, 16, 22, 25] {
            assert!(
                matches!(
                    select(&program, &call(&build, &[], Some(refused))),
                    Err(ProgramError::Denied(_))
                ),
                "{refused}"
            );
        }
        assert!(matches!(
            select(&program, &call(&build, &[], None)),
            Err(ProgramError::Invalid(_))
        ));
    }

    #[test]
    fn the_game_profile_resolves_only_in_a_call_that_carries_a_game_version() {
        let program = Program {
            id: "tool".into(),
            java: Some(ProgramJava::Profile(JavaProfile::Profile)),
            args: tokens(&["**"]),
            ..Program::default()
        };
        let args = tokens(&["-version"]);
        let versioned = Call {
            game_java: Some(21),
            ..call(&args, &[], None)
        };
        assert_eq!(select(&program, &versioned).unwrap().java, Some(21));
        assert!(matches!(
            select(&program, &call(&args, &[], None)),
            Err(ProgramError::Invalid(_))
        ));
        let naming = Call {
            game_java: Some(21),
            ..call(&args, &[], Some(21))
        };
        assert!(matches!(
            select(&program, &naming),
            Err(ProgramError::Invalid(_))
        ));
    }

    #[test]
    fn a_program_without_java_takes_no_major() {
        let program = Program {
            id: "git".into(),
            command: "git".into(),
            args: tokens(&["**"]),
            ..Program::default()
        };
        let args = tokens(&["status"]);
        assert_eq!(
            select(&program, &call(&args, &[], None)).unwrap().java,
            None
        );
        assert!(matches!(
            select(&program, &call(&args, &[], Some(21))),
            Err(ProgramError::Invalid(_))
        ));
    }
}
