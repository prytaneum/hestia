use wasmparser::{ComponentExternName, Encoding, Parser, Payload};

/// The interfaces a component imports and exports, by their full names
/// (`hestia:kernel/app@0.1.0`), read from its top-level sections without
/// compiling anything.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub imports: Vec<String>,
    pub exports: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShapeError {
    #[error("plugin.wasm is not a WebAssembly component: {0}")]
    Malformed(#[from] wasmparser::BinaryReaderError),
    #[error("plugin.wasm is a core module, not a component")]
    NotAComponent,
    /// A label that names another interface would let the linked interface
    /// differ from the one the grant was derived from.
    #[error("'{0}' names an interface under another label, which Hestia does not link")]
    Implements(String),
}

impl Shape {
    pub fn read(wasm: &[u8]) -> Result<Self, ShapeError> {
        let mut shape = Shape::default();
        let mut depth = 0usize;
        for payload in Parser::new(0).parse_all(wasm) {
            match payload? {
                Payload::Version { encoding, .. } if depth == 0 => {
                    if encoding != Encoding::Component {
                        return Err(ShapeError::NotAComponent);
                    }
                }
                Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                Payload::End(_) => depth = depth.saturating_sub(1),
                Payload::ComponentImportSection(reader) if depth == 0 => {
                    for import in reader {
                        shape.imports.push(full_name(&import?.name)?);
                    }
                }
                Payload::ComponentExportSection(reader) if depth == 0 => {
                    for export in reader {
                        shape.exports.push(full_name(&export?.name)?);
                    }
                }
                _ => {}
            }
        }
        Ok(shape)
    }
}

fn full_name(name: &ComponentExternName<'_>) -> Result<String, ShapeError> {
    match name.implements {
        Some(_) => Err(ShapeError::Implements(name.name.to_owned())),
        None => Ok(name.full_name().into_owned()),
    }
}
