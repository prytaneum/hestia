wasmtime::component::bindgen!({
    path: ["../sdk/wit/kernel", "src/api/probe/wit"],
    world: "hestia:probe/host",
    imports: { default: async },
    exports: { default: async },
    with: { "hestia:kernel/types": crate::runtime::bindings::hestia::kernel::types },
});
