# A server runtime is Java or native, and a native one never meets Java

*Applies to: [Minecraft providers](../architecture/minecraft.md)*

`ServerProfile` named a jar, a launch shape and a `java_major`, and
`provision_server` always ensured a Java runtime. Bedrock Dedicated Server is a
native executable in a zip: no jar, no Java, no JVM flags.

**`ServerProfile.runtime` is a `ServerRuntime`: `Java { jar, launch, java_major,
jvm_args }` or `Native { archive, executable, env }`.** The variant is the type,
so `ensure_java`, memory and `jvm-args` can only be reached through the Java
arm; a native profile has nothing to apply them to. `ServerInfo.javaMajor` is
`None` for a native server, `config set memory|jvm-args` on one is refused with
`ConfigKeyNotApplicable`, and the create request's Java install is optional.
`LaunchPlan` gained `env`, which carries `LD_LIBRARY_PATH=.` on Linux — BDS
ships its shared libraries beside the executable.

**Rejected.** An optional jar and optional Java fields on one struct, which leave
invalid combinations representable and push a `None` check into every flow that
used to be unconditional.
