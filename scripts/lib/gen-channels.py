#!/usr/bin/env python
"""Project every `impl Contract` / `impl Job` / `impl Topic` in crates/proto/src
into frontend/src/api/types/channels.ts: the typed channel table the frontend's
`call`, `runJob` and `onDaemonEvent` are checked against. Driven by
scripts/gen-types.sh after ts-rs has written the flat generated/ files.

Exits non-zero, naming the offender, when a block cannot be read or references a
type ts-rs did not export — a contract must never silently drop out of the
table."""

import glob
import os
import re
import sys

SRC = "crates/proto/src"
OUT = "frontend/src/api/types"
GEN = f"{OUT}/generated"

BLOCK = r"impl(?:<[^>]*>)?\s+{trait}\s+for\s+(\w+)(?:<[^>]*>)?\s*\{{(.*?)\n\}}"
CONTRACT_RE = re.compile(BLOCK.format(trait="Contract"), re.S)
JOB_RE = re.compile(BLOCK.format(trait="Job"), re.S)
TOPIC_RE = re.compile(BLOCK.format(trait="Topic"), re.S)
STRUCT_RE = re.compile(r"^pub struct (\w+)", re.M)
STRUCT_BODY_RE = re.compile(r"^pub struct (\w+) \{\n(.*?)\n\}", re.M | re.S)
FIELD_RE = re.compile(r"^pub (\w+):\s*(.+?),$")
TIMEOUT_RE = re.compile(
    r"const TIMEOUT:\s*Duration\s*=\s*Duration::from_secs\(\s*(\d+(?:\s*\*\s*\d+)*)\s*\)\s*;"
)
STRING_CONSTS = re.compile(r'pub const (\w+): &str = "([^"]*)";')

PRIMITIVES = {
    "String": "string",
    "bool": "boolean",
    "()": "null",
    "Value": "unknown",
    "serde_json::Value": "unknown",
    **{
        n: "number"
        for n in "u8 u16 u32 u64 usize i8 i16 i32 i64 isize f32 f64".split()
    },
}


class Failure(Exception):
    pass


def split_type_args(expr):
    depth, start, parts = 0, 0, []
    for i, ch in enumerate(expr):
        depth += ch == "<"
        depth -= ch == ">"
        if ch == "," and depth == 0:
            parts.append(expr[start:i])
            start = i + 1
    parts.append(expr[start:])
    return [p.strip() for p in parts]


def camel(snake):
    head, *tail = snake.split("_")
    return head + "".join(word.capitalize() for word in tail)


class Types:
    """`partial` names the structs carrying a struct-level `#[serde(default)]`:
    the daemon fills any field a request omits, so a request may omit them. ts-rs
    cannot say so, which is why a request type is emitted as `Partial<…>` — and
    why a field that is itself such a struct is made partial in turn."""

    def __init__(self):
        self.imports = set()
        self.partial = set()
        self.fields = {}
        for path in glob.glob(f"{SRC}/*.rs"):
            text = open(path).read()
            lines = text.splitlines()
            for m in STRUCT_RE.finditer(text):
                line = text.count("\n", 0, m.start())
                if self._serde_default(lines, line):
                    self.partial.add(m.group(1))
            for m in STRUCT_BODY_RE.finditer(text):
                self.fields[m.group(1)] = self._parse_fields(m.group(2))

    @staticmethod
    def _parse_fields(body):
        fields, attrs = [], []
        for line in body.splitlines():
            line = line.strip()
            if line.startswith("#["):
                attrs.append(line)
                continue
            field = FIELD_RE.match(line)
            if field:
                joined = " ".join(attrs)
                rename = re.search(r'rename\s*=\s*"([^"]+)"', joined)
                fields.append(
                    (
                        rename.group(1) if rename else camel(field.group(1)),
                        field.group(2),
                        bool(re.search(r"\bflatten\b", joined)),
                    )
                )
            if not line.startswith("//"):
                attrs = []
        return fields

    def _overrides(self, name, where):
        found = {}
        for key, expr, flatten in self.fields.get(name, []):
            expr = expr.strip()
            if flatten:
                found.update(self._overrides(expr.split("::")[-1], where))
                continue
            optional = re.fullmatch(r"Option<(.+)>", expr, re.S)
            inner = optional.group(1) if optional else expr
            lenient = self.lenient(inner, where)
            if lenient is not None:
                found[key] = lenient
        return found

    def lenient(self, expr, where):
        """The TS type of a nested field once its default structs are partial,
        or None when nothing inside it is."""
        expr = expr.strip()
        generic = re.fullmatch(r"(Vec|Option)<(.+)>", expr, re.S)
        if generic:
            inner = self.lenient(generic.group(2), where)
            if inner is None:
                return None
            return f"Array<{inner}>" if generic.group(1) == "Vec" else f"{inner} | null"
        name = expr.split("::")[-1]
        if re.fullmatch(r"(?:\w+::)*[A-Z]\w*", expr) and name in self.partial:
            return self.request(name, where)
        return None

    def request(self, name, where):
        rendered = f"Partial<{self.ts(name, where)}>"
        nested = self._overrides(name, where)
        if not nested:
            return rendered
        omitted = " | ".join(f'"{key}"' for key in nested)
        props = "; ".join(f'"{key}"?: {ts}' for key, ts in nested.items())
        return f"Omit<{rendered}, {omitted}> & {{ {props} }}"

    @staticmethod
    def _serde_default(lines, struct_line):
        i = struct_line
        while i > 0:
            i -= 1
            attr = lines[i].strip()
            if not (attr.startswith("#[") or attr.startswith("//")):
                return False
            serde = re.search(r"#\[serde\((.*)\)\]", attr)
            if serde and re.search(r"(?:^|[\s,])default(?:\s*=|\s*,|\s*$)", serde.group(1)):
                return True
        return False

    def ts(self, expr, where):
        expr = expr.strip()
        if expr in PRIMITIVES:
            return PRIMITIVES[expr]
        generic = re.fullmatch(r"(Vec|Option)<(.+)>", expr, re.S)
        if generic:
            inner = self.ts(generic.group(2), where)
            return f"Array<{inner}>" if generic.group(1) == "Vec" else f"{inner} | null"
        if re.fullmatch(r"(?:\w+::)*[A-Z]\w*", expr):
            name = expr.split("::")[-1]
            if not os.path.exists(f"{GEN}/{name}.ts"):
                raise Failure(f"{where}: `{name}` has no generated TypeScript type")
            self.imports.add(name)
            return name
        raise Failure(f"{where}: cannot translate type `{expr}`")

    def params(self, expr, where):
        rendered = self.ts(expr, where)
        name = expr.strip().split("::")[-1]
        return self.request(name, where) if name in self.partial else rendered


def assoc(body, kind, where):
    m = re.search(rf"type {kind}\s*=\s*([^;]+);", body)
    if not m:
        raise Failure(f"{where}: no `type {kind}`")
    return m.group(1)


def timeout_ms(body, where):
    m = TIMEOUT_RE.search(body)
    if m:
        seconds = 1
        for factor in m.group(1).split("*"):
            seconds *= int(factor)
        return seconds * 1000
    if "TIMEOUT" in body:
        raise Failure(
            f"{where}: TIMEOUT must be `Duration::from_secs(<int> [* <int>]…)`"
        )
    return None


def main():
    types = Types()
    channels, jobs, topics, timeouts, names = {}, {}, {}, {}, {}
    expected_contracts = 0
    expected_topics = 0

    for path in sorted(glob.glob(f"{SRC}/*.rs")):
        module = os.path.basename(path)[:-3]
        text = open(path).read()
        consts = dict(STRING_CONSTS.findall(text))
        expected_contracts += len(re.findall(r"impl\s+Contract\s+for\s", text))
        expected_topics += len(re.findall(r"impl(?:<[^>]*>)?\s+Topic\s+for\s", text))

        for m in CONTRACT_RE.finditer(text):
            name, body = m.groups()
            where = f"{module}::{name}"
            channel = re.search(r'const CHANNEL:\s*&\'static str\s*=\s*"([^"]+)"', body)
            if not channel:
                raise Failure(f"{where}: no literal CHANNEL")
            key = channel.group(1)
            if key in channels:
                raise Failure(f"{where}: duplicate channel `{key}`")
            channels[key] = (
                types.params(assoc(body, "Params", where), where),
                types.ts(assoc(body, "Result", where), where),
            )
            ms = timeout_ms(body, where)
            if ms is not None:
                timeouts[key] = ms
            names[name] = key

        for m in JOB_RE.finditer(text):
            name, body = m.groups()
            where = f"{module}::{name}"
            jobs[name] = (
                types.ts(assoc(body, "Progress", where), where),
                types.ts(assoc(body, "Done", where), where),
            )

        for m in TOPIC_RE.finditer(text):
            name, body = m.groups()
            where = f"{module}::{name}"
            topic = re.search(r"const TOPIC:\s*&'static str\s*=\s*([^;]+);", body)
            if not topic:
                raise Failure(f"{where}: no TOPIC")
            value = topic.group(1).strip()
            value = consts.get(value) if not value.startswith('"') else value.strip('"')
            if value is None:
                raise Failure(f"{where}: TOPIC is neither a literal nor a const here")
            if value in topics:
                raise Failure(f"{where}: duplicate topic `{value}`")
            topics[value] = (name, "<" in m.group(0).split("{")[0].split("for", 1)[1])

    if len(names) != expected_contracts:
        raise Failure(
            f"parsed {len(names)} contracts but the source declares {expected_contracts}"
        )
    if len(topics) != expected_topics:
        raise Failure(
            f"parsed {len(topics)} topics but the source declares {expected_topics}"
        )

    job_table = {}
    for name, payload in jobs.items():
        if name not in names:
            raise Failure(f"{name}: `impl Job` without an `impl Contract`")
        job_table[names[name]] = payload

    topic_lines = []
    for topic, (name, generic) in sorted(topics.items()):
        if generic:
            if name != "JobEvent":
                raise Failure(f"{name}: a generic topic payload is only known for JobEvent")
            topic_lines.append(f'  "{topic}": JobEvents;')
        else:
            topic_lines.append(f'  "{topic}": {types.ts(name, name)};')

    out = [
        "// Generated by scripts/gen-types.sh from the `impl Contract` blocks in crates/proto — do not edit.",
    ]
    for name in sorted(types.imports):
        out.append(f'import type {{ {name} }} from "./generated/{name}";')
    out.append('import type { JobEvent } from "./generated/JobEvent";')
    out.append("")
    out.append("export interface Channels {")
    for key in sorted(channels):
        params, result = channels[key]
        out.append(f'  "{key}": {{ params: {params}; result: {result} }};')
    out.append("}")
    out.append("")
    out.append("export interface Jobs {")
    for key in sorted(job_table):
        progress, done = job_table[key]
        out.append(f'  "{key}": {{ progress: {progress}; done: {done} }};')
    out.append("}")
    out.append("")
    out.append("export type JobEvents = {")
    out.append(
        "  [K in keyof Jobs]: JobEvent<Jobs[K]['progress'], Jobs[K]['done']> & { channel: K };"
    )
    out.append("}[keyof Jobs];")
    out.append("")
    out.append("export interface Topics {")
    out.extend(topic_lines)
    out.append("}")
    out.append("")
    out.append("export const TIMEOUTS: Partial<Record<keyof Channels, number>> = {")
    for key in sorted(timeouts):
        out.append(f'  "{key}": {timeouts[key]},')
    out.append("};")
    open(f"{OUT}/channels.ts", "w").write("\n".join(out) + "\n")
    print(
        f"wrote channels.ts: {len(channels)} channels, {len(job_table)} jobs, {len(topics)} topics"
    )


try:
    main()
except Failure as failure:
    print(f"gen-channels: {failure}", file=sys.stderr)
    sys.exit(1)
